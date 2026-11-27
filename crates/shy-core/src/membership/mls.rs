//! Workspace membership and key agreement, delegated to OpenMLS.
//!
//! MLS is used only to agree on one secret per epoch; content is encrypted
//! with keys exported from it (see `crypto`). Only the workspace admin commits,
//! so there is a single linear chain of epochs and no forks to resolve.

use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use iroh::{PublicKey, SecretKey, Signature};
use openmls::prelude::{
    tls_codec::{Deserialize as _, Serialize as _},
    *,
};
use openmls::schedule::{PreSharedKeyId, Psk};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::RustCrypto;
use openmls_sqlite_storage::{Codec, SqliteStorageProvider};
use openmls_traits::{OpenMlsProvider, random::OpenMlsRand, types::SignatureScheme};
use rusqlite::Connection;

use crate::{
    crypto::{EPOCH_KEY_LEN, EpochKey},
    storage::open_encrypted,
};

const CIPHERSUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
const SIGNATURE_SCHEME: SignatureScheme = SignatureScheme::ED25519;
const CONTENT_KEY_LABEL: &str = "shyspace content key v1";
const BINDING_LABEL: &[u8] = b"shyspace mls key binding v1";

/// What a member signs with their identity key to prove they own an MLS
/// signature key. Without it, anyone could claim another member's identity.
pub fn binding_message(mls_signature_key: &[u8]) -> Vec<u8> {
    [BINDING_LABEL, mls_signature_key].concat()
}

#[derive(Default)]
pub struct JsonCodec;

impl Codec for JsonCodec {
    type Error = serde_json::Error;

    fn to_vec<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, Self::Error> {
        serde_json::to_vec(value)
    }

    fn from_slice<T: serde::de::DeserializeOwned>(slice: &[u8]) -> Result<T, Self::Error> {
        serde_json::from_slice(slice)
    }
}

struct Provider {
    crypto: RustCrypto,
    storage: SqliteStorageProvider<JsonCodec, Connection>,
}

impl OpenMlsProvider for Provider {
    type CryptoProvider = RustCrypto;
    type RandProvider = RustCrypto;
    type StorageProvider = SqliteStorageProvider<JsonCodec, Connection>;

    fn storage(&self) -> &Self::StorageProvider {
        &self.storage
    }

    fn crypto(&self) -> &Self::CryptoProvider {
        &self.crypto
    }

    fn rand(&self) -> &Self::RandProvider {
        &self.crypto
    }
}

pub struct Mls {
    provider: Provider,
    signer: SignatureKeyPair,
    identity: PublicKey,
    group: Option<MlsGroup>,
}

pub struct CommitOutcome {
    pub removed_self: bool,
}

/// The id of the device map's pre-shared key; its value changes each commit.
const MAP_KEY_ID: &[u8] = b"shyspace device map";

/// The pre-shared keys an admin's commit must carry: the device map's when
/// its wrapper binds the map, none otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitKeys {
    None,
    DeviceMap,
}

/// MLS refused a commit that binds the device map because this device's
/// key for the map isn't the one the admin committed with.
#[derive(Debug)]
pub struct MapKeyMismatch;

impl std::fmt::Display for MapKeyMismatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("this device's list of devices doesn't match the workspace's")
    }
}

impl std::error::Error for MapKeyMismatch {}

fn keys_carried(staged: &StagedCommit) -> Result<CommitKeys> {
    let ids = staged
        .psk_proposals()
        .map(|queued| {
            let bytes = queued.psk_proposal().tls_serialize_detached()?;
            Ok(PreSharedKeyId::tls_deserialize_exact(bytes)?)
        })
        .collect::<Result<Vec<_>>>()?;
    match ids.as_slice() {
        [] => Ok(CommitKeys::None),
        [id] if matches!(id.psk(), Psk::External(external) if external.psk_id() == MAP_KEY_ID) => {
            Ok(CommitKeys::DeviceMap)
        }
        _ => bail!("the commit carries pre-shared keys shyspace doesn't use"),
    }
}

fn group_config() -> MlsGroupJoinConfig {
    MlsGroupJoinConfig::builder()
        .wire_format_policy(PURE_CIPHERTEXT_WIRE_FORMAT_POLICY)
        .use_ratchet_tree_extension(true)
        .build()
}

fn identity_of(credential: &Credential) -> Result<PublicKey> {
    let basic = BasicCredential::try_from(credential.clone()).context("not a basic credential")?;
    let bytes: &[u8; 32] = basic.identity().try_into().context("identity must be 32 bytes")?;
    Ok(PublicKey::from_bytes(bytes)?)
}

fn decode_protocol_message(bytes: &[u8]) -> Result<ProtocolMessage> {
    Ok(MlsMessageIn::tls_deserialize_exact(bytes)?.try_into_protocol_message()?)
}

impl Mls {
    /// Opens the MLS state in `db_path`. `signer_public` is the MLS signing
    /// key remembered from an earlier session; `None` creates a new one.
    pub fn open(
        db_path: &Path,
        db_key: &str,
        identity: &SecretKey,
        signer_public: Option<&[u8]>,
        group_id: Option<&[u8]>,
    ) -> Result<Self> {
        let mut storage = SqliteStorageProvider::new(open_encrypted(db_path, db_key)?);
        storage.run_migrations().context("MLS storage migrations")?;
        let provider = Provider {
            crypto: RustCrypto::default(),
            storage,
        };
        let signer = match signer_public {
            Some(public) => SignatureKeyPair::read(provider.storage(), public, SIGNATURE_SCHEME)
                .context("MLS signing key missing from storage")?,
            None => {
                let signer = SignatureKeyPair::new(SIGNATURE_SCHEME)?;
                signer.store(provider.storage())?;
                signer
            }
        };
        let group = match group_id {
            Some(id) => MlsGroup::load(provider.storage(), &GroupId::from_slice(id))?,
            None => None,
        };
        Ok(Self {
            provider,
            signer,
            identity: identity.public(),
            group,
        })
    }

    pub fn signer_public(&self) -> &[u8] {
        self.signer.public()
    }

    fn credential(&self) -> CredentialWithKey {
        CredentialWithKey {
            credential: BasicCredential::new(self.identity.as_bytes().to_vec()).into(),
            signature_key: self.signer.public().into(),
        }
    }

    fn group(&self) -> Result<&MlsGroup> {
        self.group.as_ref().context("not a member of the workspace group")
    }

    pub fn create_group(&mut self, group_id: &[u8]) -> Result<()> {
        let config = MlsGroupCreateConfig::builder()
            .ciphersuite(CIPHERSUITE)
            .wire_format_policy(PURE_CIPHERTEXT_WIRE_FORMAT_POLICY)
            .use_ratchet_tree_extension(true)
            .build();
        let group = MlsGroup::new_with_group_id(
            &self.provider,
            &self.signer,
            &config,
            GroupId::from_slice(group_id),
            self.credential(),
        )?;
        self.group = Some(group);
        Ok(())
    }

    /// A fresh key package the admin uses to add us. Its private part stays
    /// in our storage until the matching Welcome arrives.
    pub fn key_package(&self) -> Result<Vec<u8>> {
        let bundle =
            KeyPackage::builder().build(CIPHERSUITE, &self.provider, &self.signer, self.credential())?;
        Ok(MlsMessageOut::from(bundle.key_package().clone()).to_bytes()?)
    }

    /// Joins from a Welcome, but only if it is for the expected workspace and
    /// the admin in it uses the MLS key pinned by the invite. That stops a
    /// forged Welcome from luring us into someone else's group.
    pub fn join(
        &mut self,
        welcome_bytes: &[u8],
        group_id: &[u8],
        admin: &PublicKey,
        admin_mls_key: &[u8],
    ) -> Result<()> {
        let MlsMessageBodyIn::Welcome(welcome) =
            MlsMessageIn::tls_deserialize_exact(welcome_bytes)?.extract()
        else {
            bail!("expected a Welcome message");
        };
        let staged = StagedWelcome::new_from_welcome(&self.provider, &group_config(), welcome, None)?;
        ensure!(
            staged.group_context().group_id().as_slice() == group_id,
            "Welcome is for a different workspace"
        );
        let admin_is_genuine = staged.members().any(|member| {
            identity_of(&member.credential).is_ok_and(|id| id == *admin)
                && member.signature_key == admin_mls_key
        });
        ensure!(admin_is_genuine, "Welcome does not come from the invite's admin");
        self.group = Some(staged.into_group(&self.provider)?);
        Ok(())
    }

    pub fn is_active(&self) -> bool {
        self.group.as_ref().is_some_and(MlsGroup::is_active)
    }

    pub fn epoch(&self) -> Result<u64> {
        Ok(self.group()?.epoch().as_u64())
    }

    pub fn epoch_key(&self) -> Result<EpochKey> {
        let secret =
            self.group()?
                .export_secret(self.provider.crypto(), CONTENT_KEY_LABEL, &[], EPOCH_KEY_LEN)?;
        Ok(secret.try_into().expect("exported key has the requested length"))
    }

    pub fn members(&self) -> Result<Vec<PublicKey>> {
        self.group()?
            .members()
            .map(|member| identity_of(&member.credential))
            .collect()
    }

    /// A member's MLS signature key, from the group's tree.
    pub fn signature_key_of(&self, member: &PublicKey) -> Result<Vec<u8>> {
        self.group()?
            .members()
            .find(|candidate| identity_of(&candidate.credential).is_ok_and(|id| id == *member))
            .map(|found| found.signature_key)
            .context("not a member")
    }

    /// Checks who a key package claims to be, without changing anything.
    /// `binding` must be the claimed identity's signature over its MLS key.
    pub fn verify_key_package(
        &self,
        key_package_bytes: &[u8],
        binding: &[u8],
    ) -> Result<(PublicKey, KeyPackage)> {
        let MlsMessageBodyIn::KeyPackage(key_package) =
            MlsMessageIn::tls_deserialize_exact(key_package_bytes)?.extract()
        else {
            bail!("expected a key package");
        };
        let key_package = key_package.validate(self.provider.crypto(), ProtocolVersion::Mls10)?;
        let leaf = key_package.leaf_node();
        let joiner = identity_of(leaf.credential())?;
        let binding: [u8; 64] = binding.try_into().context("malformed identity binding")?;
        joiner
            .verify(
                &binding_message(leaf.signature_key().as_slice()),
                &Signature::from_bytes(&binding),
            )
            .context("key package is not signed by the identity it claims")?;
        Ok((joiner, key_package))
    }

    /// Stages adding a verified joiner. Returns `(commit, welcome)`; the
    /// caller saves the commit durably, then calls `merge_pending_commit`.
    /// `binding` goes in the commit's authenticated data (see
    /// [`super::commit::lists_hash`]); `map_key`, for a commit that binds the
    /// device map, is that map's digest as a pre-shared key.
    pub fn add_member(
        &mut self,
        key_package: KeyPackage,
        binding: &[u8],
        map_key: Option<&[u8; 32]>,
    ) -> Result<(Vec<u8>, Vec<u8>)> {
        if let Some(map_key) = map_key {
            let (commit, welcome) =
                self.commit_with_map_key(vec![key_package], Vec::new(), binding, map_key)?;
            return Ok((commit, welcome.context("an add without a Welcome")?));
        }
        let group = self.group.as_mut().context("no group")?;
        group.set_aad(binding.to_vec());
        let added = group.add_members(&self.provider, &self.signer, &[key_package]);
        let (commit, welcome, _) = added.inspect_err(|_| group.set_aad(Vec::new()))?;
        Ok((commit.to_bytes()?, welcome.to_bytes()?))
    }

    /// Stages a commit as `add_members` and `remove_members` do, plus the
    /// device map's pre-shared key. Returns the commit and any Welcome.
    fn commit_with_map_key(
        &mut self,
        adds: Vec<KeyPackage>,
        removals: Vec<LeafNodeIndex>,
        binding: &[u8],
        map_key: &[u8; 32],
    ) -> Result<(Vec<u8>, Option<Vec<u8>>)> {
        self.store_map_key(map_key)?;
        let nonce = self.provider.rand().random_vec(CIPHERSUITE.hash_length())?;
        let psk = PreSharedKeyId::external(MAP_KEY_ID.to_vec(), nonce);
        // A commit with no adds or removals exists only to change the keys,
        // which takes the admin's own leaf changing.
        let is_forcing_update = !adds.is_empty() || removals.is_empty();
        let group = self.group.as_mut().context("no group")?;
        group.set_aad(binding.to_vec());
        let staged = group
            .commit_builder()
            .propose_adds(adds)
            .propose_removals(removals)
            .force_self_update(is_forcing_update)
            .add_proposal(Proposal::PreSharedKey(Box::new(PreSharedKeyProposal::new(psk))))
            .load_psks(self.provider.storage())
            .map_err(anyhow::Error::from)
            .and_then(|builder| {
                Ok(
                    builder.build(self.provider.rand(), self.provider.crypto(), &self.signer, |_| {
                        true
                    })?,
                )
            })
            .and_then(|built| Ok(built.stage_commit(&self.provider)?));
        group.set_aad(Vec::new());
        let bundle = staged?;
        let welcome = bundle
            .to_welcome_msg()
            .map(|welcome| welcome.to_bytes())
            .transpose()?;
        Ok((bundle.into_commit().to_bytes()?, welcome))
    }

    /// Hands MLS the device map's digest this device computed, before it
    /// processes a commit that binds the map: a commit made with another
    /// value is refused, using nothing up.
    pub fn store_map_key(&self, map_key: &[u8; 32]) -> Result<()> {
        // The nonce is chosen per commit and isn't stored; the id is.
        PreSharedKeyId::external(MAP_KEY_ID.to_vec(), Vec::new()).store(&self.provider, map_key)?;
        Ok(())
    }

    pub fn has_pending_commit(&self) -> bool {
        self.group
            .as_ref()
            .is_some_and(|group| group.pending_commit().is_some())
    }

    pub fn discard_pending_commit(&mut self) -> Result<()> {
        let group = self.group.as_mut().context("no group")?;
        group.clear_pending_commit(self.provider.storage())?;
        Ok(())
    }

    pub fn merge_pending_commit(&mut self) -> Result<bool> {
        let group = self.group.as_mut().context("no group")?;
        if group.pending_commit().is_none() {
            return Ok(false);
        }
        group.merge_pending_commit(&self.provider)?;
        Ok(true)
    }

    pub fn is_member(&self, identity: &PublicKey) -> Result<bool> {
        Ok(self.members()?.contains(identity))
    }

    /// What a dishonest admin's app could stage: adds and removals in one
    /// commit, which an honest one never makes for devices.
    #[cfg(test)]
    pub(crate) fn commit_adds_and_removals(
        &mut self,
        adds: Vec<KeyPackage>,
        removals: &[PublicKey],
        binding: &[u8],
        map_key: &[u8; 32],
    ) -> Result<Vec<u8>> {
        let group = self.group.as_ref().context("no group")?;
        let leaves = removals
            .iter()
            .map(|member| {
                Ok(group
                    .members()
                    .find(|candidate| identity_of(&candidate.credential).is_ok_and(|id| id == *member))
                    .context("not a member")?
                    .index)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(self.commit_with_map_key(adds, leaves, binding, map_key)?.0)
    }

    /// The admin's own leaf's encryption key, which a key-only commit changes.
    #[cfg(test)]
    pub(crate) fn own_leaf_encryption_key(&self) -> Result<Vec<u8>> {
        Ok(postcard::to_stdvec(
            self.group()?
                .own_leaf_node()
                .context("no own leaf")?
                .encryption_key(),
        )?)
    }

    /// Stages a commit that changes nothing but the group's keys, by
    /// refreshing the admin's own leaf: keys handed to a device that was
    /// then refused stop opening what comes next. `binding` and `map_key`
    /// as for [`Self::add_member`].
    pub fn update_keys(&mut self, binding: &[u8], map_key: Option<&[u8; 32]>) -> Result<Vec<u8>> {
        if let Some(map_key) = map_key {
            return Ok(self
                .commit_with_map_key(Vec::new(), Vec::new(), binding, map_key)?
                .0);
        }
        let group = self.group.as_mut().context("no group")?;
        group.set_aad(binding.to_vec());
        let updated = group.self_update(&self.provider, &self.signer, LeafNodeParameters::default());
        let bundle = updated.inspect_err(|_| group.set_aad(Vec::new()))?;
        Ok(bundle.into_commit().to_bytes()?)
    }

    /// `binding` and `map_key` as for [`Self::add_member`].
    /// Stages one commit removing every leaf in `members`.
    pub fn remove_members(
        &mut self,
        members: &[PublicKey],
        binding: &[u8],
        map_key: Option<&[u8; 32]>,
    ) -> Result<Vec<u8>> {
        let group = self.group.as_ref().context("no group")?;
        let leaves = members
            .iter()
            .map(|member| {
                Ok(group
                    .members()
                    .find(|candidate| identity_of(&candidate.credential).is_ok_and(|id| id == *member))
                    .context("not a member")?
                    .index)
            })
            .collect::<Result<Vec<_>>>()?;
        if let Some(map_key) = map_key {
            return Ok(self.commit_with_map_key(Vec::new(), leaves, binding, map_key)?.0);
        }
        let group = self.group.as_mut().context("no group")?;
        group.set_aad(binding.to_vec());
        let removed = group.remove_members(&self.provider, &self.signer, &leaves);
        let (commit, _, _) = removed.inspect_err(|_| group.set_aad(Vec::new()))?;
        Ok(commit.to_bytes()?)
    }

    /// A commit's authenticated data, read without decrypting or staging it:
    /// staging a commit uses it up, so this is checked first. It is in clear
    /// on the encrypted commit, and MLS refuses the commit if it was changed.
    pub fn commit_binding(commit_bytes: &[u8]) -> Result<Vec<u8>> {
        let ProtocolMessage::PrivateMessage(message) = decode_protocol_message(commit_bytes)? else {
            bail!("commits are always encrypted");
        };
        Ok(message.aad().to_vec())
    }

    /// Applies a commit published by the admin. Commits from anyone else are
    /// rejected, which keeps the epoch chain linear, and so is one that
    /// doesn't carry exactly the pre-shared keys its wrapper calls for.
    pub fn process_commit(
        &mut self,
        commit_bytes: &[u8],
        admin: &PublicKey,
        keys: CommitKeys,
    ) -> Result<CommitOutcome> {
        let message = decode_protocol_message(commit_bytes)?;
        let provider = &self.provider;
        let group = self.group.as_mut().context("no group")?;
        let processed = match group.process_message(provider, message) {
            Err(ProcessMessageError::InvalidCommit(
                StageCommitError::ConfirmationTagMismatch | StageCommitError::PskError(_),
            )) if keys == CommitKeys::DeviceMap => bail!(MapKeyMismatch),
            processed => processed?,
        };
        ensure!(
            identity_of(processed.credential())? == *admin,
            "commit not signed by the workspace admin"
        );
        let ProcessedMessageContent::StagedCommitMessage(staged) = processed.into_content() else {
            bail!("expected a commit");
        };
        ensure!(
            keys_carried(&staged)? == keys,
            "the commit's pre-shared keys don't match its wrapper"
        );
        let removed_self = staged.self_removed();
        group.merge_staged_commit(provider, *staged)?;
        Ok(CommitOutcome { removed_self })
    }

    /// Encrypts `payload` for the members of the current epoch.
    pub fn seal_for_members(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let group = self.group.as_mut().context("no group")?;
        let message = group.create_message(&self.provider, &self.signer, payload)?;
        Ok(message.to_bytes()?)
    }

    pub fn open_from_admin(&mut self, bytes: &[u8], admin: &PublicKey) -> Result<Vec<u8>> {
        let message = decode_protocol_message(bytes)?;
        let group = self.group.as_mut().context("no group")?;
        let processed = group.process_message(&self.provider, message)?;
        ensure!(
            identity_of(processed.credential())? == *admin,
            "message not sent by the workspace admin"
        );
        let ProcessedMessageContent::ApplicationMessage(application) = processed.into_content() else {
            bail!("expected an application message");
        };
        Ok(application.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::TEST_DB_KEY;

    struct TestMember {
        identity: SecretKey,
        mls: Mls,
    }

    impl TestMember {
        fn new(dir: &Path, name: &str) -> Self {
            let identity = SecretKey::generate();
            let mls = Mls::open(
                &dir.join(format!("{name}.db")),
                TEST_DB_KEY,
                &identity,
                None,
                None,
            )
            .unwrap();
            Self { identity, mls }
        }

        fn id(&self) -> PublicKey {
            self.identity.public()
        }

        fn join_request(&self) -> (Vec<u8>, Vec<u8>) {
            let binding = self.identity.sign(&binding_message(self.mls.signer_public()));
            (self.mls.key_package().unwrap(), binding.to_bytes().to_vec())
        }
    }

    const GROUP: &[u8] = b"workspace-1";

    fn add(admin: &mut TestMember, joiner: &mut TestMember) -> Vec<u8> {
        let (key_package, binding) = joiner.join_request();
        let (claimed, key_package) = admin.mls.verify_key_package(&key_package, &binding).unwrap();
        assert_eq!(claimed, joiner.id());
        let (commit, welcome) = admin.mls.add_member(key_package, &[], None).unwrap();
        admin.mls.merge_pending_commit().unwrap();
        let admin_key = admin.mls.signer_public().to_vec();
        joiner.mls.join(&welcome, GROUP, &admin.id(), &admin_key).unwrap();
        commit
    }

    #[test]
    fn admin_adds_and_removes_a_member() {
        let dir = tempfile::tempdir().unwrap();
        let mut admin = TestMember::new(dir.path(), "admin");
        let mut bob = TestMember::new(dir.path(), "bob");
        admin.mls.create_group(GROUP).unwrap();
        add(&mut admin, &mut bob);
        assert_eq!(bob.mls.epoch().unwrap(), 1);
        assert_eq!(bob.mls.epoch_key().unwrap(), admin.mls.epoch_key().unwrap());

        let sealed = admin.mls.seal_for_members(b"history").unwrap();
        assert_eq!(bob.mls.open_from_admin(&sealed, &admin.id()).unwrap(), b"history");

        let removal = admin.mls.remove_members(&[bob.id()], &[], None).unwrap();
        admin.mls.merge_pending_commit().unwrap();
        let outcome = bob
            .mls
            .process_commit(&removal, &admin.id(), CommitKeys::None)
            .unwrap();
        assert!(outcome.removed_self);
        assert!(!bob.mls.is_active());
        assert_eq!(admin.mls.members().unwrap(), vec![admin.id()]);
    }

    #[test]
    fn a_commits_binding_reads_back_without_using_the_commit_up() {
        let dir = tempfile::tempdir().unwrap();
        let mut admin = TestMember::new(dir.path(), "admin");
        let mut bob = TestMember::new(dir.path(), "bob");
        let carol = TestMember::new(dir.path(), "carol");
        admin.mls.create_group(GROUP).unwrap();
        add(&mut admin, &mut bob);
        let (key_package, binding) = carol.join_request();
        let (_, key_package) = admin.mls.verify_key_package(&key_package, &binding).unwrap();
        let (commit, _) = admin.mls.add_member(key_package, b"the lists", None).unwrap();
        admin.mls.merge_pending_commit().unwrap();

        assert_eq!(Mls::commit_binding(&commit).unwrap(), b"the lists");
        bob.mls
            .process_commit(&commit, &admin.id(), CommitKeys::None)
            .unwrap();
        assert_eq!(bob.mls.epoch().unwrap(), 2);
        assert!(Mls::commit_binding(b"junk").is_err());
    }

    #[test]
    fn state_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let identity = SecretKey::generate();
        let path = dir.path().join("mls.db");
        let mut mls = Mls::open(&path, TEST_DB_KEY, &identity, None, None).unwrap();
        mls.create_group(GROUP).unwrap();
        let key = mls.epoch_key().unwrap();
        let signer_public = mls.signer_public().to_vec();
        drop(mls);

        let reopened = Mls::open(&path, TEST_DB_KEY, &identity, Some(&signer_public), Some(GROUP)).unwrap();
        assert_eq!(reopened.epoch_key().unwrap(), key);
    }

    #[test]
    fn members_reject_commits_not_signed_by_admin() {
        let dir = tempfile::tempdir().unwrap();
        let mut admin = TestMember::new(dir.path(), "admin");
        let mut bob = TestMember::new(dir.path(), "bob");
        let mut dave = TestMember::new(dir.path(), "dave");
        let carol = TestMember::new(dir.path(), "carol");
        admin.mls.create_group(GROUP).unwrap();
        add(&mut admin, &mut bob);
        let dave_commit = add(&mut admin, &mut dave);
        bob.mls
            .process_commit(&dave_commit, &admin.id(), CommitKeys::None)
            .unwrap();

        let (key_package, binding) = carol.join_request();
        let (_, key_package) = bob.mls.verify_key_package(&key_package, &binding).unwrap();
        let (rogue_commit, _) = bob.mls.add_member(key_package, &[], None).unwrap();
        let error = dave
            .mls
            .process_commit(&rogue_commit, &admin.id(), CommitKeys::None)
            .err()
            .unwrap();
        assert!(error.to_string().contains("admin"), "{error}");
    }

    #[test]
    fn rejects_key_package_claiming_someone_elses_identity() {
        let dir = tempfile::tempdir().unwrap();
        let mut admin = TestMember::new(dir.path(), "admin");
        let victim = TestMember::new(dir.path(), "victim");
        let attacker = TestMember::new(dir.path(), "attacker");
        admin.mls.create_group(GROUP).unwrap();
        // The attacker builds a key package claiming the victim's identity.
        let forged = Mls {
            identity: victim.id(),
            ..Mls::open(
                &dir.path().join("forged.db"),
                TEST_DB_KEY,
                &attacker.identity,
                None,
                None,
            )
            .unwrap()
        };
        let binding = attacker.identity.sign(&binding_message(forged.signer_public()));
        let result = admin
            .mls
            .verify_key_package(&forged.key_package().unwrap(), &binding.to_bytes());
        assert!(result.is_err());
    }

    #[test]
    fn rejects_welcome_from_an_impostor_admin() {
        let dir = tempfile::tempdir().unwrap();
        let real_admin = TestMember::new(dir.path(), "admin");
        let mut impostor = TestMember::new(dir.path(), "impostor");
        let mut joiner = TestMember::new(dir.path(), "joiner");
        impostor.mls.create_group(GROUP).unwrap();
        let (key_package, binding) = joiner.join_request();
        let (_, key_package) = impostor.mls.verify_key_package(&key_package, &binding).unwrap();
        let (_, welcome) = impostor.mls.add_member(key_package, &[], None).unwrap();
        impostor.mls.merge_pending_commit().unwrap();
        let pinned_key = real_admin.mls.signer_public().to_vec();
        assert!(
            joiner
                .mls
                .join(&welcome, GROUP, &real_admin.id(), &pinned_key)
                .is_err()
        );
    }
}

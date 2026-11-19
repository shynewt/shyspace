//! Workspace membership and key agreement, delegated to OpenMLS.
//!
//! MLS is used only to agree on one secret per epoch; content is encrypted
//! with keys exported from it (see `crypto`). Only the workspace admin commits,
//! so there is a single linear chain of epochs and no forks to resolve.

use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use iroh::{PublicKey, SecretKey, Signature};
use openmls::prelude::{tls_codec::Deserialize as _, *};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::RustCrypto;
use openmls_sqlite_storage::{Codec, SqliteStorageProvider};
use openmls_traits::{OpenMlsProvider, types::SignatureScheme};
use rusqlite::Connection;

use crate::crypto::{EPOCH_KEY_LEN, EpochKey};

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
        identity: &SecretKey,
        signer_public: Option<&[u8]>,
        group_id: Option<&[u8]>,
    ) -> Result<Self> {
        let mut storage = SqliteStorageProvider::new(Connection::open(db_path)?);
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
    pub fn add_member(&mut self, key_package: KeyPackage) -> Result<(Vec<u8>, Vec<u8>)> {
        let group = self.group.as_mut().context("no group")?;
        let (commit, welcome, _) = group.add_members(&self.provider, &self.signer, &[key_package])?;
        Ok((commit.to_bytes()?, welcome.to_bytes()?))
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

    pub fn remove_member(&mut self, member: &PublicKey) -> Result<Vec<u8>> {
        let group = self.group.as_ref().context("no group")?;
        let leaf = group
            .members()
            .find(|candidate| identity_of(&candidate.credential).is_ok_and(|id| id == *member))
            .context("not a member")?
            .index;
        let group = self.group.as_mut().context("no group")?;
        let (commit, _, _) = group.remove_members(&self.provider, &self.signer, &[leaf])?;
        Ok(commit.to_bytes()?)
    }

    /// Applies a commit published by the admin. Commits from anyone else are
    /// rejected, which keeps the epoch chain linear.
    pub fn process_commit(&mut self, commit_bytes: &[u8], admin: &PublicKey) -> Result<CommitOutcome> {
        let message = decode_protocol_message(commit_bytes)?;
        let provider = &self.provider;
        let group = self.group.as_mut().context("no group")?;
        let processed = group.process_message(provider, message)?;
        ensure!(
            identity_of(processed.credential())? == *admin,
            "commit not signed by the workspace admin"
        );
        let ProcessedMessageContent::StagedCommitMessage(staged) = processed.into_content() else {
            bail!("expected a commit");
        };
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

    struct TestMember {
        identity: SecretKey,
        mls: Mls,
    }

    impl TestMember {
        fn new(dir: &Path, name: &str) -> Self {
            let identity = SecretKey::generate();
            let mls = Mls::open(&dir.join(format!("{name}.db")), &identity, None, None).unwrap();
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
        let (commit, welcome) = admin.mls.add_member(key_package).unwrap();
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

        let removal = admin.mls.remove_member(&bob.id()).unwrap();
        admin.mls.merge_pending_commit().unwrap();
        let outcome = bob.mls.process_commit(&removal, &admin.id()).unwrap();
        assert!(outcome.removed_self);
        assert!(!bob.mls.is_active());
        assert_eq!(admin.mls.members().unwrap(), vec![admin.id()]);
    }

    #[test]
    fn state_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let identity = SecretKey::generate();
        let path = dir.path().join("mls.db");
        let mut mls = Mls::open(&path, &identity, None, None).unwrap();
        mls.create_group(GROUP).unwrap();
        let key = mls.epoch_key().unwrap();
        let signer_public = mls.signer_public().to_vec();
        drop(mls);

        let reopened = Mls::open(&path, &identity, Some(&signer_public), Some(GROUP)).unwrap();
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
        bob.mls.process_commit(&dave_commit, &admin.id()).unwrap();

        let (key_package, binding) = carol.join_request();
        let (_, key_package) = bob.mls.verify_key_package(&key_package, &binding).unwrap();
        let (rogue_commit, _) = bob.mls.add_member(key_package).unwrap();
        let error = dave.mls.process_commit(&rogue_commit, &admin.id()).err().unwrap();
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
            ..Mls::open(&dir.path().join("forged.db"), &attacker.identity, None, None).unwrap()
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
        let (_, welcome) = impostor.mls.add_member(key_package).unwrap();
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

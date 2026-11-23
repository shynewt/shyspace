# Setting up storage

A workspace needs one S3-compatible bucket. It only ever holds ciphertext, but every member holds its keys, so set it up so those keys can't destroy anything for good. This takes about 15 minutes, once, by the admin.

## What protection means here

- **Versioning on.** A delete or overwrite keeps the old version.
- **Members' key can't delete old versions.** It may put, get, list and delete (a delete only hides), never `DeleteObjectVersion`. Keep the provider's owner key for yourself: it can purge for real (a GDPR erasure, junk from a vandal).
- **Old versions expire only where things are meant to go:** after 30 days under `<workspace id>/files/` (deleted files, which then are really gone) and `<workspace id>/recovery/` (the overwritten admin snapshot); never under `<workspace id>/ops/` or `<workspace id>/mls/` (the history).
- **No whole-bucket lock.** shyspace deletes and overwrites some of its own objects.

The workspace id is shown in Settings › Storage › How to protect your bucket. Every object sits under it, and rules must name it.

Settings › Storage reports "Partly protected" for this setup: it can see versioning, but not what the key may do.

## Scaleway (recommended)

A French provider (EU jurisdiction), with no request fees.

1. Create an account at scaleway.com and turn on two-factor login.
2. **Object Storage › Create a bucket:** a private bucket in Paris, Amsterdam or Warsaw. Turn on **versioning**.
3. **IAM › Applications:** create an application named `shyspace-members`, give it an IAM policy with Object Storage access in this project, and create an **API key** for it. Note the access key and secret.
4. **The bucket's policy** narrows that: an action is allowed only if both the IAM policy and the bucket policy allow it. Allow the application `s3:GetObject`, `s3:PutObject`, `s3:DeleteObject` and `s3:ListBucket` on the bucket and its objects, and nothing else; Scaleway allows only what a policy lists, so leaving out `s3:DeleteObjectVersion` is enough. Include yourself (your user) as a principal too, or you lock yourself out of the bucket.
5. **Lifecycle rules:** one rule with prefix `<workspace id>/files/` and one with `<workspace id>/recovery/`, each deleting noncurrent versions after 30 days. No rule for `ops/` or `mls/`, and never a rule with an empty prefix.
6. In shyspace, create the workspace with endpoint `https://s3.<region>.scw.cloud` (for example `https://s3.fr-par.scw.cloud`), region `fr-par`, the bucket name, and the application's key. The workspace id appears in Settings once it exists, so add the lifecycle rules (step 5) right after.
7. Check with the members' key: deleting an object works, and it still shows under versions; deleting a version is refused. These steps come from Scaleway's documentation and haven't been tried on a real account yet, so do this check.

## Others

- **AWS S3:** the same with IAM: versioning on, a members' policy without `s3:DeleteObjectVersion`, and lifecycle expiry of noncurrent versions under `files/`. Don't set an Object Lock default retention: it covers the whole bucket. A US company, with request fees.
- **Cloudflare R2:** no versioning. Add a bucket lock rule for `<workspace id>/ops/` only: an R2 lock refuses overwrites, and commits under `mls/` are repaired by overwriting, so locking `mls/` would let junk put at the next commit slot block membership changes for good. Deleted files are gone at once, so back up `files/` another way. A US company, with request fees.
- **MinIO (self-hosted):** `mc version enable`, and a members' policy without `s3:DeleteObjectVersion`.

Backblaze B2 can do this too, but its privacy notice describes sharing identifiers with advertising networks, so we don't recommend it for a privacy product.

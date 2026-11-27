# Vendored libraries

`rustls-platform-verifier-<version>.aar` is the Kotlin half of the
[rustls-platform-verifier](https://github.com/rustls/rustls-platform-verifier) crate, which the
core uses to check HTTPS certificates through the phone's trust store. It's published only on a
branch of that repository, which can change, so it's kept here instead of downloaded.

Its version must match `rustls-platform-verifier-android` in `Cargo.lock`; the build fails if no
file here does.

## Where it comes from

Upstream builds it with `ci/package_android_release.sh` (from the `android` folder) and stores it
on the `maven-archive` branch; see their `admin/RELEASING.md`. The copy here is byte for byte the
one at a fixed commit of that branch:

| File | Built at (main) | Stored at (maven-archive) | SHA-256 |
|---|---|---|---|
| `rustls-platform-verifier-0.2.0.aar` | `18f5c53e41498f510c701b65e35ca08083600949` | `69c6eb55d6293763857df52f926a3aa4ed6af0e5` | `aa021794230fbc2f0be355999e2cf67398dd563de066a2e17001ffbd0b69101b` |

To check it:

```sh
curl -sSfL https://raw.githubusercontent.com/rustls/rustls-platform-verifier/69c6eb55d6293763857df52f926a3aa4ed6af0e5/android-release-support/maven/org/rustls/rustls-platform-verifier/0.2.0/rustls-platform-verifier-0.2.0.aar | sha256sum
```

To update it after `cargo update` moves that crate to a new version: find the `maven-archive`
commit that added the version (`Prepare Maven release x.y.z`), download the AAR from that commit
as above, record both commits and its SHA-256 here and in `platformVerifierSha256` in `app/build.gradle.kts` (the build checks it), delete the old file, and run the on-phone
tests (`make device-test`).

#!/bin/sh
# Builds the Rust core for iPhones and the Apple silicon simulator, its Swift bindings, and the
# XCFramework the ShyCore package links (both generated, never committed). Runs on a Mac with
# Xcode and rustup: `apps/ios/build-core.sh` for development (with the in-memory bucket the tests
# use), `apps/ios/build-core.sh release` for a release.
set -eu
cd "$(dirname "$0")/../.."
mode=${1:-debug}
case $mode in
  debug) profile=dev; dir=debug; features=vendored-openssl,memory-bucket ;;
  release) profile=release; dir=release; features=vendored-openssl ;;
  *) echo "usage: $0 [debug|release]" >&2; exit 2 ;;
esac
# The Rust objects must target the app's minimum iOS, or the link warns about every one of them.
export IPHONEOS_DEPLOYMENT_TARGET=26.0
targets="aarch64-apple-ios aarch64-apple-ios-sim"
for target in $targets; do
  cargo rustc -p shy-ffi --lib --crate-type staticlib --profile "$profile" --target "$target" --features "$features"
done

cargo build -p shy-ffi --features "bindgen,$features" --bin uniffi-bindgen-swift
bindgen=target/debug/uniffi-bindgen-swift
library=target/aarch64-apple-ios/$dir/libshy_ffi.a
generated=target/ios-bindings
rm -rf "$generated"
"$bindgen" "$library" "$generated/Sources" --swift-sources
# The headers and module map sit in a folder of their own, so they can't clash with another
# library's in the app's include path.
"$bindgen" "$library" "$generated/Headers/ShyCoreFFI" --headers
"$bindgen" "$library" "$generated/Headers/ShyCoreFFI" --modulemap --module-name ShyCoreFFI --modulemap-filename module.modulemap

package=apps/ios/ShyCore
rm -rf "$package/ShyCoreFFI.xcframework" "$package/Sources/ShyCore"
mkdir -p "$package/Sources/ShyCore"
cp "$generated"/Sources/*.swift "$package/Sources/ShyCore/"
set --
for target in $targets; do
  set -- "$@" -library "target/$target/$dir/libshy_ffi.a" -headers "$generated/Headers"
done
xcodebuild -create-xcframework "$@" -output "$package/ShyCoreFFI.xcframework"

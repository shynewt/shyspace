.PHONY: image debug release test unit check screens device-test gradle dependency-checksums wrapper debug-keystore clean

# Everything builds inside a container, so the host needs no JDK, Android SDK, NDK or Android
# Rust targets. The whole repository is mounted, since the app builds the Rust core.
CONTAINER ?= podman
IMAGE := shyspace-android-build
REPO := $(abspath ../..)
RUST_VERSION := $(shell sed -n 's/^channel = "\(.*\)"/\1/p' $(REPO)/rust-toolchain.toml)
# Downloads (Gradle, Maven, crates, Robolectric's Android runtimes in .m2) are shared by every worktree; build output stays in each
# worktree's own target/android and app/build.
CACHE := /var/tmp/shyspace-android-cache
TTY := $(shell [ -t 0 ] && echo -t)
HOST_UID := $(shell id -u)
HOST_GID := $(shell id -g)
# Rootless podman already maps the container's root to the invoking user, and a chown to
# that user's uid would map it onto a subordinate uid the host user cannot write.
HAND_BACK := $(if $(findstring podman,$(shell $(CONTAINER) --version 2>/dev/null)),true,chown -Rf $(HOST_UID):$(HOST_GID))
# SELinux (Fedora) would deny the container the mounted repository; relabelling the whole
# repository instead would be slow and change its labels.
RUN = $(CONTAINER) run --rm --security-opt label=disable \
	-v $(REPO):/repo \
	-v $(CACHE)/gradle:/cache/gradle \
	-v $(CACHE)/cargo:/cache/cargo \
	-v $(CACHE)/m2:/root/.m2 \
	-e GRADLE_USER_HOME=/cache/gradle \
	-e CARGO_HOME=/cache/cargo \
	-w /repo/apps/android $(IMAGE)

define gradle
	mkdir -p $(CACHE)/gradle $(CACHE)/cargo $(CACHE)/m2
	$(RUN) sh -c "export PATH=/opt/cargo/bin:\$$PATH; ./gradlew --no-daemon $(1); status=\$$?; $(HAND_BACK) /repo/apps/android /repo/target/android /cache /root/.m2 2>/dev/null; exit \$$status"
endef

image:
	$(CONTAINER) build --build-arg RUST_VERSION=$(RUST_VERSION) -t $(IMAGE) .

debug: image
	$(call gradle,assembleDebug)
	@ls -lh app/build/outputs/apk/debug/*.apk

release: image
	$(call gradle,assembleRelease)

test: image
	$(call gradle,testDebugUnitTest)

# Runs the unit tests matching TESTS, e.g. make unit TESTS='*CoreBinding*'.
unit: image
	$(call gradle,testDebugUnitTest --tests '$(TESTS)')

# What must pass before a commit that touches the app. Kotlin compiles from scratch here: an
# incremental compile once missed a test that no longer built after the bindings changed, and
# the check passed without running it.
check: image
	$(call gradle,testDebugUnitTest testReleaseUnitTest lintDebug assembleDebug assembleDebugAndroidTest -Pkotlin.incremental=false)

# Renders every screen, light and dark, at a phone's size to app/build/outputs/roborazzi/.
screens: image
	$(call gradle,recordRoborazziDebug --tests '*ScreenImages*')

# Runs the instrumented tests on a phone or emulator that adb can see on this machine: a phone
# on USB (USB debugging on) or one paired for wireless debugging. It updates the debug app and
# leaves it installed (Gradle would otherwise uninstall it, data and all, from every profile),
# then removes only the test app.
# A container sees only the USB devices there when it starts, so this waits on the host for a
# device offering adb first (a USB interface of class 255, subclass 66).
WAIT_FOR_PHONE := echo 'Waiting up to 2 minutes for a phone with USB debugging (unlock it)'; \
	timeout 120 sh -c 'until cat /sys/bus/usb/devices/*:*/uevent 2>/dev/null | grep -q "^INTERFACE=255/66/"; do sleep 1; done'

device-test: image
	mkdir -p $(CACHE)/gradle $(CACHE)/cargo $(CACHE)/adb
	@$(WAIT_FOR_PHONE)
	$(CONTAINER) run --rm --security-opt label=disable --network host --device /dev/bus/usb \
		-v $(REPO):/repo -v $(CACHE)/gradle:/cache/gradle -v $(CACHE)/cargo:/cache/cargo -v $(CACHE)/m2:/root/.m2 -v $(CACHE)/adb:/root/.android \
		-e GRADLE_USER_HOME=/cache/gradle -e CARGO_HOME=/cache/cargo -w /repo/apps/android $(IMAGE) \
		sh -c "export PATH=/opt/cargo/bin:\$$PATH; adb start-server && adb devices && \
			timeout 30 adb wait-for-device && \
			./gradlew --no-daemon connectedDebugAndroidTest -Pandroid.injected.androidTest.leaveApksInstalledAfterRun=true; \
			status=\$$?; adb uninstall dev.shynewt.shyspace.debug.test || status=1; exit \$$status"

# Writes gradle/verification-metadata.xml afresh: the SHA-256 of every artifact Gradle downloads,
# which it checks on every build. Run it after adding or updating a dependency, and read the
# diff: only the artifacts you meant to change should be new. It starts from an empty cache of
# its own, since Gradle fetches some metadata only when its cache is empty, and a warm cache
# would leave those out (a fresh clone would then fail verification). Maven Central may answer
# 429 if run again right away: wait a while between runs.
dependency-checksums: CACHE = /var/tmp/shyspace-android-cache-checksums
dependency-checksums: image
	rm -rf $(CACHE) gradle/verification-metadata.xml
	$(call gradle,--write-verification-metadata sha256 testDebugUnitTest testReleaseUnitTest lintDebug assembleDebug assembleRelease assembleDebugAndroidTest)
	rm -rf $(CACHE)

# Runs any Gradle tasks, e.g. make gradle ARGS='lintDebug --stacktrace'.
gradle: image
	$(call gradle,$(ARGS))

# Regenerates the Gradle wrapper from a distribution whose checksum matches Gradle's.
GRADLE_VERSION := 9.9.2
GRADLE_SHA256 := 971fcd7150ca4ba04852c0b2ca0d2d0ab9468e14b03473eab19528809914c76b
wrapper: image
	mkdir -p $(CACHE)
	test -f $(CACHE)/gradle-$(GRADLE_VERSION)-bin.zip || curl -sSfL https://services.gradle.org/distributions/gradle-$(GRADLE_VERSION)-bin.zip -o $(CACHE)/gradle-$(GRADLE_VERSION)-bin.zip
	echo "$(GRADLE_SHA256)  $(CACHE)/gradle-$(GRADLE_VERSION)-bin.zip" | sha256sum -c -
	rm -rf $(CACHE)/gradle-$(GRADLE_VERSION) && cd $(CACHE) && unzip -q gradle-$(GRADLE_VERSION)-bin.zip
	$(CONTAINER) run --rm --security-opt label=disable -v $(REPO):/repo -v $(CACHE):/cache -w /repo/apps/android $(IMAGE) \
		/cache/gradle-$(GRADLE_VERSION)/bin/gradle --no-daemon wrapper --gradle-version $(GRADLE_VERSION) \
		--distribution-type bin --gradle-distribution-sha256-sum $(GRADLE_SHA256)

# Makes the fixed, public debug key once; it is committed so every build signs alike.
debug-keystore: image
	$(CONTAINER) run --rm --security-opt label=disable -v $(REPO):/repo -w /repo/apps/android $(IMAGE) \
		keytool -genkeypair -keystore debug.keystore -storepass android -alias androiddebugkey \
		-keypass android -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=Android Debug,O=Android,C=US"

clean:
	rm -rf app/build build .gradle .kotlin

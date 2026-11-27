// swift-tools-version: 6.2
// The Rust core for the iOS app: the XCFramework and Swift bindings that ../build-core.sh makes.
import PackageDescription

let package = Package(
    name: "ShyCore",
    platforms: [.iOS(.v26)],
    products: [.library(name: "ShyCore", targets: ["ShyCore"])],
    targets: [
        .binaryTarget(name: "ShyCoreFFI", path: "ShyCoreFFI.xcframework"),
        .target(
            name: "ShyCore",
            dependencies: ["ShyCoreFFI"],
            // UniFFI's generated Swift isn't Sendable-correct for Swift 6 yet (uniffi-rs #2448).
            swiftSettings: [.swiftLanguageMode(.v5)],
            // What the Rust core calls in the system: certificate checks, network changes and
            // settings (iroh watches the path through Network), DNS.
            linkerSettings: [
                .linkedFramework("Network"),
                .linkedFramework("Security"),
                .linkedFramework("SystemConfiguration"),
                .linkedFramework("CoreFoundation"),
                .linkedLibrary("resolv"),
            ]
        ),
        .testTarget(name: "ShyCoreTests", dependencies: ["ShyCore"]),
    ]
)

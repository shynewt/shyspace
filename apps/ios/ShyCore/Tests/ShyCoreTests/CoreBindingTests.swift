import Foundation
import ShyCore
import Testing

/// Keys in memory, keeping the core's contract: no overwrite, and nil only for "none".
final class MemoryKeyStore: DeviceKeyStore, @unchecked Sendable {
    private var secrets: [String: Data] = [:]
    private let lock = NSLock()

    func read(account: String) throws -> Data? {
        lock.withLock { secrets[account] }
    }

    func create(account: String, secret: Data) throws {
        try lock.withLock {
            guard secrets[account] == nil else { throw DeviceKeyStoreError.Failed(reason: "\(account) already has a secret") }
            secrets[account] = secret
        }
    }

    func delete(account: String) throws {
        lock.withLock { _ = secrets.removeValue(forKey: account) }
    }
}

@Test func aMessageSentThroughTheBindingsComesBackFromTheCore() async throws {
    let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: folder) }
    let app = ShyApp.withMemoryBucket(dataDir: folder.path, keyStore: MemoryKeyStore(), networking: .offline)
    let unused = BucketSettings(endpoint: "http://unused", region: "us-east-1", bucket: "unused", accessKeyId: "unused", secretAccessKey: "unused-secret")

    let acme = try await app.createWorkspace(workspaceName: "Acme", memberName: "Ada", bucket: unused)
    let general = try #require(try await acme.channels().first { $0.name == "general" })
    _ = try await acme.sendMessage(channel: general.id, text: "hello from Swift", replyTo: nil)

    let messages = try await acme.messages(channel: general.id, before: nil, limit: 50)
    #expect(messages.map(\.text) == ["hello from Swift"])
    #expect(messages.first?.authorName == "Ada")
}

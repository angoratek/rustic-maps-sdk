//! A loopback fixture server for the type-ahead UI cases
//! (`docs/design/Autocomplete.md`): it answers the SDK's suggest requests
//! from committed fixture documents, records what the provider asked for,
//! and can delay one answer so a superseded request is provably dropped.
//!
//! The application under test reaches it through the launch-environment
//! deployment override and no request leaves the simulator.

import Foundation
import Network

/// One canned answer: the status, the JSON body and how long to wait before
/// sending it.
struct FixtureSearchResponse {
    /// The HTTP status the SDK sees.
    let statusCode: Int
    /// The response body (a committed fixture document).
    let body: Data
    /// The delay before the answer is sent; a superseded request uses it to
    /// arrive after the newer query's answer.
    let delaySeconds: TimeInterval
}

/// One request the server received: what the provider asked and when.
struct FixtureSearchRequest {
    /// The request path (the provider appends its endpoint to the base URL).
    let path: String
    /// The typed query (`q`).
    let queryText: String
    /// The candidate cap the provider requested (`limit`), when present.
    let resultLimit: String?
    /// When the request arrived.
    let receivedAt: Date
}

/// A minimal HTTP/1.1 server over one loopback listener.
final class FixtureSearchServer {
    /// Answers one request from its path and query items.
    typealias Responder = (String, [String: String]) -> FixtureSearchResponse

    private let listener: NWListener
    private let queue = DispatchQueue(label: "fixture-search-server")
    private let requestLock = NSLock()
    private var recordedRequests: [FixtureSearchRequest] = []
    private let responder: Responder
    private var startFailure: Swift.Error?
    private let readySemaphore = DispatchSemaphore(value: 0)

    /// Creates a listener on an ephemeral loopback port.
    init(responder: @escaping Responder) throws {
        self.responder = responder
        let parameters = NWParameters.tcp
        parameters.allowLocalEndpointReuse = true
        listener = try NWListener(using: parameters, on: .any)
    }

    /// Starts listening and returns the assigned port.
    @discardableResult
    func start() throws -> UInt16 {
        listener.stateUpdateHandler = { [weak self] state in
            guard let self else { return }
            switch state {
            case .ready:
                self.readySemaphore.signal()
            case let .failed(error):
                self.startFailure = error
                self.readySemaphore.signal()
            default:
                break
            }
        }
        listener.newConnectionHandler = { [weak self] connection in
            self?.accept(connection)
        }
        listener.start(queue: queue)
        guard readySemaphore.wait(timeout: .now() + 5) == .success else {
            throw FixtureSearchServerError.startTimedOut
        }
        if let startFailure {
            throw startFailure
        }
        guard let port = listener.port?.rawValue else {
            throw FixtureSearchServerError.missingPort
        }
        return port
    }

    /// Stops listening; the test owns the lifecycle.
    func stop() {
        listener.cancel()
    }

    /// The requests recorded so far, oldest first.
    func requests() -> [FixtureSearchRequest] {
        requestLock.lock()
        defer { requestLock.unlock() }
        return recordedRequests
    }

    /// Polls until a request carries the given query text.
    @discardableResult
    func waitForRequest(queryText: String, timeout: TimeInterval = 10) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if requests().contains(where: { $0.queryText == queryText }) {
                return true
            }
            Thread.sleep(forTimeInterval: 0.02)
        }
        return requests().contains(where: { $0.queryText == queryText })
    }

    private func accept(_ connection: NWConnection) {
        connection.start(queue: queue)
        receive(on: connection, buffer: Data())
    }

    private func receive(
        on connection: NWConnection,
        buffer: Data
    ) {
        connection.receive(
            minimumIncompleteLength: 1,
            maximumLength: 64 * 1024
        ) { [weak self] data, _, isComplete, error in
            guard let self else {
                connection.cancel()
                return
            }
            var accumulated = buffer
            if let data {
                accumulated.append(data)
            }
            if accumulated.range(of: Data("\r\n\r\n".utf8)) != nil {
                self.respond(to: accumulated, on: connection)
                return
            }
            if isComplete || error != nil {
                connection.cancel()
                return
            }
            self.receive(on: connection, buffer: accumulated)
        }
    }

    private func respond(to requestData: Data, on connection: NWConnection) {
        let requestHead = String(decoding: requestData, as: UTF8.self)
        let requestLine = requestHead.split(separator: "\r\n").first ?? ""
        let requestParts = requestLine.split(separator: " ")
        let target = requestParts.count > 1 ? String(requestParts[1]) : "/"
        let components = URLComponents(string: target)
        let path = components?.path ?? "/"
        var queryItems: [String: String] = [:]
        for item in components?.queryItems ?? [] {
            queryItems[item.name] = item.value ?? ""
        }
        let recordedRequest = FixtureSearchRequest(
            path: path,
            queryText: queryItems["q"] ?? "",
            resultLimit: queryItems["limit"],
            receivedAt: Date()
        )
        requestLock.lock()
        recordedRequests.append(recordedRequest)
        requestLock.unlock()

        let response = responder(path, queryItems)
        let reason = response.statusCode == 200 ? "OK" : "Error"
        let header = "HTTP/1.1 \(response.statusCode) \(reason)\r\n"
            + "Content-Type: application/json\r\n"
            + "Content-Length: \(response.body.count)\r\n"
            + "Connection: close\r\n\r\n"
        var payload = Data(header.utf8)
        payload.append(response.body)
        queue.asyncAfter(deadline: .now() + max(0, response.delaySeconds)) {
            connection.send(
                content: payload,
                completion: .contentProcessed { _ in
                    connection.cancel()
                }
            )
        }
    }
}

/// A fixture server failure the test cannot continue past.
enum FixtureSearchServerError: Swift.Error {
    /// The listener never reached the ready state.
    case startTimedOut
    /// The listener reported no port after becoming ready.
    case missingPort
    /// A committed fixture document was missing from the test bundle.
    case fixtureMissing(documentName: String)
}

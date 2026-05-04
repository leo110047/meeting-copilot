import AVFoundation
import CoreGraphics
import CoreMedia
import Foundation
import ScreenCaptureKit
import Speech

public typealias NativeSpeechCallback = @convention(c) (UnsafePointer<CChar>?, UnsafeMutableRawPointer?) -> Void
public typealias NativeSpeechReleaseContext = @convention(c) (UnsafeMutableRawPointer?) -> Void

struct BridgeTranscriptLine: Codable {
    let kind: String
    let text: String
    let isFinal: Bool
    let confidence: Double
    let language: String
    let source: String
    let startedAtMs: Int64
    let endedAtMs: Int64
}

struct BridgeErrorLine: Codable {
    let kind: String
    let message: String
    let source: String
}

private let bridgeLock = NSLock()
private var nextBridgeHandle: Int32 = 1
private var activeBridges: [Int32: NativeSpeechBridge] = [:]

private enum NativeSpeechStatusFlag {
    static let speechAuthorized: Int32 = 1 << 0
    static let speechDenied: Int32 = 1 << 1
    static let speechRestricted: Int32 = 1 << 2
    static let speechNotDetermined: Int32 = 1 << 3
    static let recognizerAvailable: Int32 = 1 << 4
    static let microphoneAuthorized: Int32 = 1 << 5
    static let microphoneDenied: Int32 = 1 << 6
    static let microphoneRestricted: Int32 = 1 << 7
    static let microphoneNotDetermined: Int32 = 1 << 8
    static let screenCapturePreflight: Int32 = 1 << 9
    static let requiresMicrophone: Int32 = 1 << 10
    static let requiresScreenCapture: Int32 = 1 << 11
    static let macOS13OrNewer: Int32 = 1 << 12
}

private func hasScreenCaptureAccess() -> Bool {
    if #available(macOS 10.15, *) {
        return CGPreflightScreenCaptureAccess()
    }
    return true
}

private func requestScreenCaptureAccessIfNeeded() -> Bool {
    if hasScreenCaptureAccess() {
        return true
    }
    if #available(macOS 10.15, *) {
        return CGRequestScreenCaptureAccess()
    }
    return true
}

private func requestMicrophoneAccessIfNeeded() -> Bool {
    let status = AVCaptureDevice.authorizationStatus(for: .audio)
    if status == .authorized {
        return true
    }
    if status != .notDetermined {
        return false
    }
    let semaphore = DispatchSemaphore(value: 0)
    var granted = false
    AVCaptureDevice.requestAccess(for: .audio) { nextGranted in
        granted = nextGranted
        semaphore.signal()
    }
    semaphore.wait()
    return granted
}

private func requestSpeechAuthorizationSync() -> SFSpeechRecognizerAuthorizationStatus {
    let status = SFSpeechRecognizer.authorizationStatus()
    if status != .notDetermined {
        return status
    }
    let semaphore = DispatchSemaphore(value: 0)
    var resolved = status
    SFSpeechRecognizer.requestAuthorization { nextStatus in
        resolved = nextStatus
        semaphore.signal()
    }
    semaphore.wait()
    return resolved
}

private func stringFromPointer(_ pointer: UnsafePointer<CChar>?, fallback: String) -> String {
    guard let pointer else { return fallback }
    return String(cString: pointer)
}

private func nativeSpeechStatus(source: String, language: String) -> Int32 {
    var status: Int32 = 0
    switch SFSpeechRecognizer.authorizationStatus() {
    case .authorized:
        status |= NativeSpeechStatusFlag.speechAuthorized
    case .denied:
        status |= NativeSpeechStatusFlag.speechDenied
    case .restricted:
        status |= NativeSpeechStatusFlag.speechRestricted
    case .notDetermined:
        status |= NativeSpeechStatusFlag.speechNotDetermined
    @unknown default:
        break
    }
    if SFSpeechRecognizer(locale: Locale(identifier: language))?.isAvailable ?? false {
        status |= NativeSpeechStatusFlag.recognizerAvailable
    }
    switch AVCaptureDevice.authorizationStatus(for: .audio) {
    case .authorized:
        status |= NativeSpeechStatusFlag.microphoneAuthorized
    case .denied:
        status |= NativeSpeechStatusFlag.microphoneDenied
    case .restricted:
        status |= NativeSpeechStatusFlag.microphoneRestricted
    case .notDetermined:
        status |= NativeSpeechStatusFlag.microphoneNotDetermined
    @unknown default:
        break
    }
    if hasScreenCaptureAccess() {
        status |= NativeSpeechStatusFlag.screenCapturePreflight
    }
    if source == "mic" {
        status |= NativeSpeechStatusFlag.requiresMicrophone
    }
    if source == "system" {
        status |= NativeSpeechStatusFlag.requiresScreenCapture
    }
    if #available(macOS 13.0, *) {
        status |= NativeSpeechStatusFlag.macOS13OrNewer
    }
    return status
}

private func nativeSpeechReady(source: String, status: Int32) -> Bool {
    let speechReady = (status & NativeSpeechStatusFlag.speechAuthorized) != 0
        && (status & NativeSpeechStatusFlag.recognizerAvailable) != 0
    let micReady = source != "mic" || (status & NativeSpeechStatusFlag.microphoneAuthorized) != 0
    let screenReady = source != "system"
        || ((status & NativeSpeechStatusFlag.screenCapturePreflight) != 0
            && (status & NativeSpeechStatusFlag.macOS13OrNewer) != 0)
    return speechReady && micReady && screenReady
}

@_cdecl("meeting_copilot_native_speech_status")
public func meetingCopilotNativeSpeechStatus(
    _ sourcePointer: UnsafePointer<CChar>?,
    _ languagePointer: UnsafePointer<CChar>?
) -> Int32 {
    let source = stringFromPointer(sourcePointer, fallback: "mic")
    let language = stringFromPointer(languagePointer, fallback: "zh-TW")
    return nativeSpeechStatus(source: source, language: language)
}

@_cdecl("meeting_copilot_native_speech_health")
public func meetingCopilotNativeSpeechHealth(
    _ sourcePointer: UnsafePointer<CChar>?,
    _ languagePointer: UnsafePointer<CChar>?
) -> Int32 {
    let source = stringFromPointer(sourcePointer, fallback: "mic")
    let language = stringFromPointer(languagePointer, fallback: "zh-TW")
    let status = nativeSpeechStatus(source: source, language: language)
    return nativeSpeechReady(source: source, status: status) ? 1 : 0
}

@_cdecl("meeting_copilot_native_speech_request_permissions")
public func meetingCopilotNativeSpeechRequestPermissions(
    _ sourcePointer: UnsafePointer<CChar>?,
    _ languagePointer: UnsafePointer<CChar>?
) -> Int32 {
    let source = stringFromPointer(sourcePointer, fallback: "mic")
    let language = stringFromPointer(languagePointer, fallback: "zh-TW")
    let _ = requestSpeechAuthorizationSync()
    let _ = source != "mic" || requestMicrophoneAccessIfNeeded()
    let _ = source != "system" || requestScreenCaptureAccessIfNeeded()
    let status = nativeSpeechStatus(source: source, language: language)
    return nativeSpeechReady(source: source, status: status) ? 1 : 0
}

@_cdecl("meeting_copilot_native_speech_start")
public func meetingCopilotNativeSpeechStart(
    _ sourcePointer: UnsafePointer<CChar>?,
    _ languagePointer: UnsafePointer<CChar>?,
    _ callback: NativeSpeechCallback?,
    _ context: UnsafeMutableRawPointer?,
    _ releaseContext: NativeSpeechReleaseContext?
) -> Int32 {
    guard let callback else {
        releaseContext?(context)
        return -1
    }
    let source = stringFromPointer(sourcePointer, fallback: "mic")
    let language = stringFromPointer(languagePointer, fallback: "zh-TW")
    guard source == "mic" || source == "system" else {
        releaseContext?(context)
        return -2
    }
    guard let bridge = NativeSpeechBridge(source: source, language: language, callback: callback, context: context, releaseContext: releaseContext) else {
        releaseContext?(context)
        return -3
    }
    do {
        try bridge.start()
    } catch {
        bridge.emitError(error.localizedDescription)
        return -4
    }
    bridgeLock.lock()
    let handle = nextBridgeHandle
    nextBridgeHandle += 1
    activeBridges[handle] = bridge
    bridgeLock.unlock()
    return handle
}

@_cdecl("meeting_copilot_native_speech_stop")
public func meetingCopilotNativeSpeechStop(_ handle: Int32) {
    bridgeLock.lock()
    let bridge = activeBridges.removeValue(forKey: handle)
    bridgeLock.unlock()
    bridge?.stop()
}

final class NativeSpeechBridge: NSObject {
    private let source: String
    private let language: String
    private let callback: NativeSpeechCallback
    private let context: UnsafeMutableRawPointer?
    private let releaseContext: NativeSpeechReleaseContext?
    private let recognizer: SFSpeechRecognizer
    private let startedAt = Date()
    private var lastText = ""
    private var recognitionTask: SFSpeechRecognitionTask?
    private var recognitionRequest: SFSpeechAudioBufferRecognitionRequest?
    private var audioEngine: AVAudioEngine?
    private var stream: Any?

    init?(source: String, language: String, callback: @escaping NativeSpeechCallback, context: UnsafeMutableRawPointer?, releaseContext: NativeSpeechReleaseContext?) {
        guard let recognizer = SFSpeechRecognizer(locale: Locale(identifier: language)) else {
            return nil
        }
        self.source = source
        self.language = language
        self.callback = callback
        self.context = context
        self.releaseContext = releaseContext
        self.recognizer = recognizer
        super.init()
    }

    deinit {
        releaseContext?(context)
    }

    func start() throws {
        let speechStatus = requestSpeechAuthorizationSync()
        guard speechStatus == .authorized else {
            throw NSError(domain: "MeetingCopilotSpeechBridge", code: 2, userInfo: [NSLocalizedDescriptionKey: "speech recognition permission is required"])
        }
        guard recognizer.isAvailable else {
            throw NSError(domain: "MeetingCopilotSpeechBridge", code: 3, userInfo: [NSLocalizedDescriptionKey: "speech recognizer is not available"])
        }
        if source == "mic", !requestMicrophoneAccessIfNeeded() {
            throw NSError(domain: "MeetingCopilotSpeechBridge", code: 8, userInfo: [NSLocalizedDescriptionKey: "microphone permission is required"])
        }
        let request = SFSpeechAudioBufferRecognitionRequest()
        request.shouldReportPartialResults = true
        request.addsPunctuation = true
        recognitionRequest = request
        recognitionTask = recognizer.recognitionTask(with: request) { [weak self] result, error in
            guard let self else { return }
            if let result {
                self.emit(result: result)
            }
            if let error {
                self.emitError(error.localizedDescription)
            }
        }
        if source == "system" {
            try startSystemAudioCapture()
        } else {
            try startMicCapture()
        }
    }

    private func startMicCapture() throws {
        let engine = AVAudioEngine()
        let inputNode = engine.inputNode
        let format = inputNode.outputFormat(forBus: 0)
        inputNode.removeTap(onBus: 0)
        inputNode.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] buffer, _ in
            self?.recognitionRequest?.append(buffer)
        }
        engine.prepare()
        try engine.start()
        audioEngine = engine
    }

    private func startSystemAudioCapture() throws {
        guard requestScreenCaptureAccessIfNeeded() else {
            throw NSError(domain: "MeetingCopilotSpeechBridge", code: 4, userInfo: [NSLocalizedDescriptionKey: "screen and system audio recording permission is required"])
        }
        if #available(macOS 13.0, *) {
            Task {
                do {
                    try await self.startScreenCaptureKitStream()
                } catch {
                    self.emitError(error.localizedDescription)
                }
            }
        } else {
            throw NSError(domain: "MeetingCopilotSpeechBridge", code: 5, userInfo: [NSLocalizedDescriptionKey: "system audio capture requires macOS 13 or newer"])
        }
    }

    @available(macOS 13.0, *)
    private func startScreenCaptureKitStream() async throws {
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
        guard let display = content.displays.first else {
            throw NSError(domain: "MeetingCopilotSpeechBridge", code: 6, userInfo: [NSLocalizedDescriptionKey: "no display available for ScreenCaptureKit audio"])
        }
        let filter = SCContentFilter(display: display, excludingWindows: [])
        let configuration = SCStreamConfiguration()
        configuration.capturesAudio = true
        configuration.excludesCurrentProcessAudio = true
        configuration.sampleRate = 16_000
        configuration.channelCount = 1
        // ScreenCaptureKit requires video dimensions even when we only consume audio.
        configuration.width = 2
        configuration.height = 2
        configuration.minimumFrameInterval = CMTime(value: 1, timescale: 1)
        let stream = SCStream(filter: filter, configuration: configuration, delegate: self)
        try stream.addStreamOutput(self, type: .audio, sampleHandlerQueue: DispatchQueue(label: "meeting-copilot.main-system-audio"))
        self.stream = stream
        try await stream.startCapture()
    }

    func stop() {
        recognitionRequest?.endAudio()
        recognitionTask?.finish()
        if let engine = audioEngine {
            engine.stop()
            engine.inputNode.removeTap(onBus: 0)
        }
        if #available(macOS 13.0, *), let stream = stream as? SCStream {
            Task {
                try? await stream.stopCapture()
                self.stream = nil
            }
        }
    }

    private func emit(result: SFSpeechRecognitionResult) {
        let text = result.bestTranscription.formattedString.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        guard result.isFinal || text != lastText else { return }
        lastText = text
        let endedAt = Int64(Date().timeIntervalSince(startedAt) * 1000)
        let startedAtMs = max(0, endedAt - 3000)
        let confidenceValues = result.bestTranscription.segments.map { Double($0.confidence) }.filter { $0 > 0 }
        let confidence = confidenceValues.isEmpty ? 0.55 : confidenceValues.reduce(0, +) / Double(confidenceValues.count)
        emitLine(BridgeTranscriptLine(
            kind: "transcript",
            text: text,
            isFinal: result.isFinal,
            confidence: confidence,
            language: language,
            source: source,
            startedAtMs: startedAtMs,
            endedAtMs: endedAt
        ))
    }

    func emitError(_ message: String) {
        emitLine(BridgeErrorLine(kind: "error", message: message, source: source))
    }

    private func emitLine<T: Encodable>(_ value: T) {
        let encoder = JSONEncoder()
        guard let data = try? encoder.encode(value), let line = String(data: data, encoding: .utf8) else {
            return
        }
        line.withCString { pointer in
            callback(pointer, context)
        }
    }
}

@available(macOS 13.0, *)
extension NativeSpeechBridge: SCStreamDelegate, SCStreamOutput {
    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .audio, sampleBuffer.isValid else { return }
        recognitionRequest?.appendAudioSampleBuffer(sampleBuffer)
    }
}

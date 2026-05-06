import AVFoundation
import CoreAudio
import CoreGraphics
import CoreMedia
import Darwin
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
    let code: String?
}

struct BridgeDiagnosticLine: Codable {
    let kind: String
    let source: String
    let code: String
    let message: String
    let rms: Double?
    let peak: Double?
}

struct BridgeWhisperJob: Codable {
    let file: String
    let language: String
    let source: String
    let route: String
    let startedAtMs: Int64
    let endedAtMs: Int64
}

private let bridgeLock = NSLock()
private var nextBridgeHandle: Int32 = 1
private protocol NativeBridgeSession: AnyObject {
    func stop()
}
private var activeBridges: [Int32: NativeBridgeSession] = [:]

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

private func classifySpeechError(_ error: NSError) -> String {
    if error.domain == "kAFAssistantErrorDomain", [203, 1110].contains(error.code) {
        return "no_speech_detected"
    }
    if let message = error.userInfo[NSLocalizedDescriptionKey] as? String,
       let code = classifySpeechErrorMessage(message) {
        return code
    }
    if error.domain == "MeetingCopilotSpeechBridge" {
        switch error.code {
        case 1, 2:
            return "speech_permission_required"
        case 4:
            return "screen_recording_permission"
        case 8:
            return "microphone_permission_required"
        default:
            break
        }
    }
    return "\(error.domain):\(error.code)"
}

private func classifySpeechErrorMessage(_ message: String) -> String? {
    let lowered = message.lowercased()
    if lowered.contains("no speech detected")
        || message.contains("未偵測到語音")
        || message.contains("未检测到语音") {
        return "no_speech_detected"
    }
    if lowered.contains("recognition request was canceled")
        || lowered.contains("recognition request was cancelled")
        || lowered.contains("recognition request canceled")
        || lowered.contains("recognition request cancelled") {
        return "recognition_request_canceled"
    }
    if lowered.contains("screen") && lowered.contains("permission") {
        return "screen_recording_permission"
    }
    if lowered.contains("microphone") && lowered.contains("permission") {
        return "microphone_permission_required"
    }
    return nil
}

private func audioDiagnosticsEnabled() -> Bool {
    let value = ProcessInfo.processInfo.environment["MEETING_COPILOT_AUDIO_DIAGNOSTICS"] ?? ""
    return ["1", "true", "yes"].contains(value.lowercased())
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

private func nativeAudioReady(source: String, status: Int32) -> Bool {
    let micReady = source != "mic" || (status & NativeSpeechStatusFlag.microphoneAuthorized) != 0
    let screenReady = source != "system"
        || ((status & NativeSpeechStatusFlag.screenCapturePreflight) != 0
            && (status & NativeSpeechStatusFlag.macOS13OrNewer) != 0)
    return micReady && screenReady
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

@_cdecl("meeting_copilot_native_audio_request_permissions")
public func meetingCopilotNativeAudioRequestPermissions(
    _ sourcePointer: UnsafePointer<CChar>?,
    _ languagePointer: UnsafePointer<CChar>?
) -> Int32 {
    let source = stringFromPointer(sourcePointer, fallback: "mic")
    let language = stringFromPointer(languagePointer, fallback: "zh-TW")
    let _ = source != "mic" || requestMicrophoneAccessIfNeeded()
    let _ = source != "system" || requestScreenCaptureAccessIfNeeded()
    let status = nativeSpeechStatus(source: source, language: language)
    return nativeAudioReady(source: source, status: status) ? 1 : 0
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
    guard source == "mic" || source == "system" || source == "mixed" else {
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
        bridge.emitError(error)
        return -4
    }
    bridgeLock.lock()
    let handle = nextBridgeHandle
    nextBridgeHandle += 1
    activeBridges[handle] = bridge
    bridgeLock.unlock()
    return handle
}

@_cdecl("meeting_copilot_native_speech_start_whisper")
public func meetingCopilotNativeSpeechStartWhisper(
    _ sourcePointer: UnsafePointer<CChar>?,
    _ languagePointer: UnsafePointer<CChar>?,
    _ runnerPointer: UnsafePointer<CChar>?,
    _ modelPointer: UnsafePointer<CChar>?,
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
    let runner = stringFromPointer(runnerPointer, fallback: "")
    let model = stringFromPointer(modelPointer, fallback: "")
    guard source == "mic" || source == "system" || source == "mixed" else {
        releaseContext?(context)
        return -2
    }
    let bridge: NativeWhisperBridge
    do {
        bridge = try NativeWhisperBridge(source: source, language: language, runner: runner, model: model, callback: callback, context: context, releaseContext: releaseContext)
    } catch {
        releaseContext?(context)
        return -4
    }
    do {
        try bridge.start()
        bridgeLock.lock()
        let handle = nextBridgeHandle
        nextBridgeHandle += 1
        activeBridges[handle] = bridge
        bridgeLock.unlock()
        return handle
    } catch {
        bridge.emitError(error.localizedDescription, code: "local_whisper_start")
        bridge.stop()
        return -4
    }
}

@_cdecl("meeting_copilot_native_speech_stop")
public func meetingCopilotNativeSpeechStop(_ handle: Int32) {
    bridgeLock.lock()
    let bridge = activeBridges.removeValue(forKey: handle)
    bridgeLock.unlock()
    bridge?.stop()
}

final class NativeSpeechBridge: NSObject, NativeBridgeSession {
    private let source: String
    private let language: String
    private let callback: NativeSpeechCallback
    private let context: UnsafeMutableRawPointer?
    private let releaseContext: NativeSpeechReleaseContext?
    private let recognizer: SFSpeechRecognizer
    private let startedAt = Date()
    private let recognitionQueue = DispatchQueue(label: "meeting-copilot.recognition-state")
    private var lastText = ""
    private var recognitionTask: SFSpeechRecognitionTask?
    private var recognitionRequest: SFSpeechAudioBufferRecognitionRequest?
    private var recognitionRestartWorkItem: DispatchWorkItem?
    private var audioEngine: AVAudioEngine?
    private var stream: Any?
    private var isStopping = false
    private var recognitionHasStarted = false
    private var recognitionGeneration = 0
    private var lastRecoverableRestartAt = Date.distantPast
    private var lastRecoverableErrorEmitAt = Date.distantPast
    private var lastAudioDiagnosticAt = Date.distantPast

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
        recognitionQueue.sync {
            createRecognitionRequestOnQueue()
        }
        if source == "system" {
            try startSystemAudioCapture()
        } else {
            try startMicCapture()
        }
    }

    private func createRecognitionRequestOnQueue() {
        recognitionGeneration += 1
        let request = SFSpeechAudioBufferRecognitionRequest()
        request.shouldReportPartialResults = true
        request.addsPunctuation = true
        recognitionRequest = request
    }

    private func finishRecognitionOnQueue() {
        recognitionGeneration += 1
        recognitionRequest?.endAudio()
        recognitionTask?.finish()
        recognitionTask = nil
        recognitionRequest = nil
        recognitionHasStarted = false
    }

    private func startRecognitionTaskOnQueue() {
        guard !isStopping else { return }
        guard let request = recognitionRequest else { return }
        guard !recognitionHasStarted else { return }
        let generation = recognitionGeneration
        recognitionHasStarted = true
        recognitionTask = recognizer.recognitionTask(with: request) { [weak self] result, error in
            guard let self else { return }
            self.recognitionQueue.async { [weak self] in
                guard let self, !self.isStopping, generation == self.recognitionGeneration else { return }
                if let result {
                    self.emit(result: result)
                }
                if let error {
                    self.handleRecognitionErrorOnQueue(error)
                }
            }
        }
    }

    private func handleRecognitionErrorOnQueue(_ error: Error) {
        let nsError = error as NSError
        let code = classifySpeechError(nsError)
        let recoverable = isRecoverableRecognitionError(code)
        if !recoverable || Date().timeIntervalSince(lastRecoverableErrorEmitAt) >= 5 {
            emitError(nsError.localizedDescription, code: code)
            if recoverable {
                lastRecoverableErrorEmitAt = Date()
            }
        }
        if recoverable {
            restartRecognitionAfterRecoverableErrorOnQueue()
        }
    }

    private func isRecoverableRecognitionError(_ code: String) -> Bool {
        code == "no_speech_detected" || code == "recognition_request_canceled"
    }

    private func restartRecognitionAfterRecoverableErrorOnQueue() {
        guard !isStopping else { return }
        let delay = max(0.25, 2.0 - Date().timeIntervalSince(lastRecoverableRestartAt))
        recognitionRestartWorkItem?.cancel()
        let workItem = DispatchWorkItem { [weak self] in
            guard let self, !self.isStopping else { return }
            self.lastRecoverableRestartAt = Date()
            self.recognitionRestartWorkItem = nil
            self.finishRecognitionOnQueue()
            self.createRecognitionRequestOnQueue()
        }
        recognitionRestartWorkItem = workItem
        recognitionQueue.asyncAfter(deadline: .now() + delay, execute: workItem)
    }

    private func startMicCapture() throws {
        let engine = AVAudioEngine()
        let inputNode = engine.inputNode
        let format = inputNode.outputFormat(forBus: 0)
        inputNode.removeTap(onBus: 0)
        inputNode.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] buffer, _ in
            guard let self else { return }
            let level = self.measureAudioLevel(buffer)
            guard let copiedBuffer = self.copyAudioPCMBuffer(buffer) else { return }
            self.recognitionQueue.async { [weak self] in
                guard let self, !self.isStopping else { return }
                self.emitAudioDiagnosticIfNeeded(level)
                self.recognitionRequest?.append(copiedBuffer)
                if level.rms >= 0.01 || level.peak >= 0.03 {
                    self.startRecognitionTaskOnQueue()
                }
            }
        }
        engine.prepare()
        try engine.start()
        audioEngine = engine
    }

    private func copyAudioPCMBuffer(_ buffer: AVAudioPCMBuffer) -> AVAudioPCMBuffer? {
        guard let copied = AVAudioPCMBuffer(pcmFormat: buffer.format, frameCapacity: buffer.frameLength) else {
            return nil
        }
        copied.frameLength = buffer.frameLength
        let sourceBuffers = UnsafeMutableAudioBufferListPointer(buffer.mutableAudioBufferList)
        let destinationBuffers = UnsafeMutableAudioBufferListPointer(copied.mutableAudioBufferList)
        guard sourceBuffers.count == destinationBuffers.count else { return nil }
        for index in 0..<sourceBuffers.count {
            let source = sourceBuffers[index]
            var destination = destinationBuffers[index]
            guard let sourceData = source.mData, let destinationData = destination.mData else {
                return nil
            }
            memcpy(destinationData, sourceData, Int(source.mDataByteSize))
            destination.mDataByteSize = source.mDataByteSize
            destinationBuffers[index] = destination
        }
        return copied
    }

    private func measureAudioLevel(_ buffer: AVAudioPCMBuffer) -> (rms: Double, peak: Double) {
        guard let channels = buffer.floatChannelData else { return (0, 0) }
        let channelCount = Int(buffer.format.channelCount)
        let frameLength = Int(buffer.frameLength)
        guard channelCount > 0, frameLength > 0 else { return (0, 0) }
        var sumSquares = 0.0
        var peak = 0.0
        var sampleCount = 0
        for channelIndex in 0..<channelCount {
            let samples = channels[channelIndex]
            for frameIndex in 0..<frameLength {
                let sample = Double(samples[frameIndex])
                let absSample = abs(sample)
                peak = max(peak, absSample)
                sumSquares += sample * sample
                sampleCount += 1
            }
        }
        guard sampleCount > 0 else { return (0, peak) }
        return (sqrt(sumSquares / Double(sampleCount)), peak)
    }

    private func emitAudioDiagnosticIfNeeded(_ level: (rms: Double, peak: Double)) {
        guard audioDiagnosticsEnabled() else { return }
        let now = Date()
        guard now.timeIntervalSince(lastAudioDiagnosticAt) >= 2 else { return }
        lastAudioDiagnosticAt = now
        emitLine(BridgeDiagnosticLine(
            kind: "audio_diagnostic",
            source: source,
            code: "audio_input_level",
            message: String(format: "audio input rms=%.5f peak=%.5f", level.rms, level.peak),
            rms: level.rms,
            peak: level.peak
        ))
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
                    self.emitError(error)
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
        recognitionQueue.sync {
            isStopping = true
            recognitionRestartWorkItem?.cancel()
            recognitionRestartWorkItem = nil
            finishRecognitionOnQueue()
        }
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

    func emitError(_ error: Error) {
        let nsError = error as NSError
        emitError(nsError.localizedDescription, code: classifySpeechError(nsError))
    }

    func emitError(_ message: String, code: String? = nil) {
        emitLine(BridgeErrorLine(kind: "error", message: message, source: source, code: code ?? classifySpeechErrorMessage(message)))
    }

    @available(macOS 13.0, *)
    fileprivate func appendSystemAudioSampleBuffer(_ sampleBuffer: CMSampleBuffer) {
        recognitionQueue.async { [weak self] in
            guard let self, !self.isStopping else { return }
            self.recognitionRequest?.appendAudioSampleBuffer(sampleBuffer)
            self.startRecognitionTaskOnQueue()
        }
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
        appendSystemAudioSampleBuffer(sampleBuffer)
    }
}

final class NativeWhisperBridge: NSObject, NativeBridgeSession {
    private let source: String
    private let language: String
    private let runner: String
    private let model: String
    private let bridgeId = UUID().uuidString
    private let callback: NativeSpeechCallback
    private let context: UnsafeMutableRawPointer?
    private let releaseContext: NativeSpeechReleaseContext?
    private let startedAt = Date()
    private let queue = DispatchQueue(label: "meeting-copilot.whisper-bridge")
    private var audioEngine: AVAudioEngine?
    private var stream: Any?
    private var process: Process?
    private var stdinPipe: Pipe?
    private let outputGroup = DispatchGroup()
    private var sourceBuffers: [String: WhisperSourceBuffer] = [:]
    private var chunkIndex = 0
    private var isStopping = false

    init(source: String, language: String, runner: String, model: String, callback: @escaping NativeSpeechCallback, context: UnsafeMutableRawPointer?, releaseContext: NativeSpeechReleaseContext?) throws {
        guard !runner.isEmpty else {
            throw NSError(domain: "MeetingCopilotSpeechBridge", code: 20, userInfo: [NSLocalizedDescriptionKey: "missing Whisper runner path"])
        }
        guard !model.isEmpty else {
            throw NSError(domain: "MeetingCopilotSpeechBridge", code: 21, userInfo: [NSLocalizedDescriptionKey: "missing Whisper model path"])
        }
        self.source = source
        self.language = language
        self.runner = runner
        self.model = model
        self.callback = callback
        self.context = context
        self.releaseContext = releaseContext
        super.init()
    }

    deinit {
        stopSynchronouslyForDeinit()
        releaseContext?(context)
    }

    func start() throws {
        try startRunner()
        if source == "mixed" {
            try startMixedCapture()
        } else if source == "system" {
            try startSystemAudioCapture()
        } else {
            try startMicCapture()
        }
    }

    private func startRunner() throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: runner)
        process.arguments = ["--serve", "--model", model]
        let stdinPipe = Pipe()
        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()
        process.standardInput = stdinPipe
        process.standardOutput = stdoutPipe
        process.standardError = stderrPipe
        try process.run()
        self.process = process
        self.stdinPipe = stdinPipe
        forwardPipe(stdoutPipe, asError: false)
        forwardPipe(stderrPipe, asError: true)
    }

    private func forwardPipe(_ pipe: Pipe, asError: Bool) {
        let group = outputGroup
        group.enter()
        DispatchQueue.global(qos: .utility).async { [weak self] in
            defer { group.leave() }
            var pending = ""
            while true {
                let data = pipe.fileHandleForReading.availableData
                if data.isEmpty { break }
                guard let text = String(data: data, encoding: .utf8), !text.isEmpty else { continue }
                pending.append(text)
                let hasTrailingNewline = pending.hasSuffix("\n")
                var parts = pending.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
                pending = hasTrailingNewline ? "" : (parts.popLast() ?? "")
                for line in parts where !line.isEmpty {
                    if asError {
                        self?.emitError(line, code: "local_whisper_runner")
                    } else {
                        self?.emitRawLine(line)
                    }
                }
            }
            if !pending.isEmpty {
                if asError {
                    self?.emitError(pending, code: "local_whisper_runner")
                } else {
                    self?.emitRawLine(pending)
                }
            }
        }
    }

    private func startMicCapture() throws {
        let engine = AVAudioEngine()
        let inputNode = engine.inputNode
        let format = inputNode.outputFormat(forBus: 0)
        inputNode.removeTap(onBus: 0)
        inputNode.installTap(onBus: 0, bufferSize: 4096, format: format) { [weak self] buffer, _ in
            guard let self, let pcm = whisperPcm16(from: buffer) else { return }
            self.append(samples: pcm, source: "mic")
        }
        engine.prepare()
        try engine.start()
        audioEngine = engine
    }

    private func startMixedCapture() throws {
        try startMicCapture()
        try startSystemAudioCapture()
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
                    self.emitError(error.localizedDescription, code: "screen_capture")
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
        configuration.width = 2
        configuration.height = 2
        configuration.minimumFrameInterval = CMTime(value: 1, timescale: 1)
        let stream = SCStream(filter: filter, configuration: configuration, delegate: self)
        try stream.addStreamOutput(self, type: .audio, sampleHandlerQueue: DispatchQueue(label: "meeting-copilot.whisper-bridge-system-audio"))
        self.stream = stream
        try await stream.startCapture()
    }

    func stop() {
        stopAudioInputs()
        queue.async { [self] in
            guard !isStopping else { return }
            isStopping = true
            for source in Array(sourceBuffers.keys) {
                flushLocked(source: source, force: true)
            }
            try? stdinPipe?.fileHandleForWriting.close()
            stdinPipe = nil
            if let process, process.isRunning {
                waitForRunnerExit(process, timeoutSeconds: 10)
            }
            process = nil
        }
    }

    private func stopSynchronouslyForDeinit() {
        queue.sync {
            guard !isStopping else { return }
            isStopping = true
            for source in Array(sourceBuffers.keys) {
                flushLocked(source: source, force: true)
            }
            try? stdinPipe?.fileHandleForWriting.close()
            stdinPipe = nil
            if let process, process.isRunning {
                process.terminate()
            }
            process = nil
        }
        stopAudioInputs()
    }

    private func stopAudioInputs() {
        if let engine = audioEngine {
            engine.stop()
            engine.inputNode.removeTap(onBus: 0)
            audioEngine = nil
        }
        if #available(macOS 13.0, *), let stream = stream as? SCStream {
            Task { try? await stream.stopCapture() }
            self.stream = nil
        }
    }

    private func waitForRunnerExit(_ process: Process, timeoutSeconds: TimeInterval) {
        let semaphore = DispatchSemaphore(value: 0)
        DispatchQueue.global(qos: .utility).async {
            process.waitUntilExit()
            semaphore.signal()
        }
        if semaphore.wait(timeout: .now() + timeoutSeconds) == .timedOut {
            emitError("Whisper runner did not finish before stop timeout", code: "local_whisper_stop_timeout")
            process.terminate()
            process.waitUntilExit()
        }
        _ = outputGroup.wait(timeout: .now() + 3)
    }

    fileprivate func appendSystemAudioSampleBuffer(_ sampleBuffer: CMSampleBuffer) {
        guard let pcm = whisperPcm16(from: sampleBuffer) else { return }
        append(samples: pcm, source: "system")
    }

    private func append(samples newSamples: [Int16], source: String) {
        guard !newSamples.isEmpty else { return }
        queue.async { [weak self] in
            guard let self, !self.isStopping else { return }
            var buffer = self.sourceBuffers[source] ?? WhisperSourceBuffer()
            if buffer.samples.isEmpty {
                buffer.chunkStartedAtMs = self.elapsedMs()
            }
            buffer.samples.append(contentsOf: newSamples)
            self.sourceBuffers[source] = buffer
            self.flushLocked(source: source, force: false)
        }
    }

    private func flushLocked(source: String, force: Bool) {
        guard var buffer = sourceBuffers[source] else { return }
        let chunkSampleCount = 16_000 * 3
        let minimumSampleCount = force ? 4_000 : 16_000
        while buffer.samples.count >= chunkSampleCount || (force && buffer.samples.count >= minimumSampleCount) {
            let count = min(buffer.samples.count, chunkSampleCount)
            let chunk = Array(buffer.samples.prefix(count))
            buffer.samples.removeFirst(count)
            let started = buffer.chunkStartedAtMs
            let ended = started + Int64(Double(chunk.count) / 16_000.0 * 1000)
            buffer.chunkStartedAtMs = ended
            send(chunk: chunk, source: source, startedAtMs: started, endedAtMs: ended)
        }
        sourceBuffers[source] = buffer
    }

    private func send(chunk: [Int16], source: String, startedAtMs: Int64, endedAtMs: Int64) {
        chunkIndex += 1
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("meeting-copilot-whisper-\(bridgeId)-\(source)-\(chunkIndex).wav")
        do {
            try writeWhisperPcm16Wav(samples: chunk, to: url)
            let job = BridgeWhisperJob(file: url.path, language: language, source: source, route: source, startedAtMs: startedAtMs, endedAtMs: endedAtMs)
            let data = try JSONEncoder().encode(job)
            guard let input = stdinPipe?.fileHandleForWriting else {
                throw NSError(domain: "MeetingCopilotSpeechBridge", code: 22, userInfo: [NSLocalizedDescriptionKey: "Whisper runner stdin is unavailable"])
            }
            input.write(data)
            input.write(Data("\n".utf8))
        } catch {
            emitError("Whisper chunk transcription failed: \(error.localizedDescription)", code: "local_whisper_chunk")
        }
    }

    private func elapsedMs() -> Int64 {
        Int64(Date().timeIntervalSince(startedAt) * 1000)
    }

    private func emitRawLine(_ line: String) {
        line.withCString { pointer in
            callback(pointer, context)
        }
    }

    func emitError(_ message: String, code: String? = nil) {
        let value = BridgeErrorLine(kind: "error", message: message, source: source, code: code)
        let encoder = JSONEncoder()
        guard let data = try? encoder.encode(value), let line = String(data: data, encoding: .utf8) else { return }
        emitRawLine(line)
    }
}

private struct WhisperSourceBuffer {
    var samples: [Int16] = []
    var chunkStartedAtMs: Int64 = 0
}

@available(macOS 13.0, *)
extension NativeWhisperBridge: SCStreamDelegate, SCStreamOutput {
    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .audio, sampleBuffer.isValid else { return }
        appendSystemAudioSampleBuffer(sampleBuffer)
    }
}

func whisperPcm16(from buffer: AVAudioPCMBuffer) -> [Int16]? {
    let channelCount = max(1, Int(buffer.format.channelCount))
    let frameCount = Int(buffer.frameLength)
    guard frameCount > 0 else { return [] }
    var mono: [Float] = []
    mono.reserveCapacity(frameCount)
    if let channels = buffer.floatChannelData {
        for frame in 0..<frameCount {
            var sample: Float = 0
            for channel in 0..<channelCount {
                sample += channels[channel][frame]
            }
            mono.append(sample / Float(channelCount))
        }
    } else if let channels = buffer.int16ChannelData {
        for frame in 0..<frameCount {
            var sample: Float = 0
            for channel in 0..<channelCount {
                sample += Float(channels[channel][frame]) / 32768.0
            }
            mono.append(sample / Float(channelCount))
        }
    } else {
        return nil
    }
    return whisperResampleToPcm16(mono, inputRate: buffer.format.sampleRate)
}

func whisperPcm16(from sampleBuffer: CMSampleBuffer) -> [Int16]? {
    guard let formatDescription = CMSampleBufferGetFormatDescription(sampleBuffer),
          let streamDescription = CMAudioFormatDescriptionGetStreamBasicDescription(formatDescription)
    else { return nil }
    let asbd = streamDescription.pointee
    let frameCount = CMSampleBufferGetNumSamples(sampleBuffer)
    guard frameCount > 0 else { return [] }
    var blockBuffer: CMBlockBuffer?
    var audioBufferList = AudioBufferList()
    let status = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
        sampleBuffer,
        bufferListSizeNeededOut: nil,
        bufferListOut: &audioBufferList,
        bufferListSize: MemoryLayout<AudioBufferList>.size,
        blockBufferAllocator: kCFAllocatorDefault,
        blockBufferMemoryAllocator: kCFAllocatorDefault,
        flags: 0,
        blockBufferOut: &blockBuffer
    )
    guard status == noErr else { return nil }
    let buffers = UnsafeMutableAudioBufferListPointer(&audioBufferList)
    let channelCount = max(1, Int(asbd.mChannelsPerFrame))
    var mono: [Float] = []
    mono.reserveCapacity(frameCount)
    for frame in 0..<frameCount {
        var sample: Float = 0
        if buffers.count == 1, let data = buffers[0].mData {
            for channel in 0..<channelCount {
                sample += whisperSample(data, index: frame * channelCount + channel, asbd: asbd)
            }
        } else {
            for channel in 0..<min(channelCount, buffers.count) {
                guard let data = buffers[channel].mData else { continue }
                sample += whisperSample(data, index: frame, asbd: asbd)
            }
        }
        mono.append(sample / Float(channelCount))
    }
    return whisperResampleToPcm16(mono, inputRate: asbd.mSampleRate)
}

func whisperSample(_ data: UnsafeMutableRawPointer, index: Int, asbd: AudioStreamBasicDescription) -> Float {
    if asbd.mFormatFlags & kAudioFormatFlagIsFloat != 0 {
        return max(-1, min(1, data.assumingMemoryBound(to: Float.self)[index]))
    }
    if asbd.mBitsPerChannel == 16 {
        return Float(data.assumingMemoryBound(to: Int16.self)[index]) / 32768.0
    }
    if asbd.mBitsPerChannel == 32 {
        return Float(data.assumingMemoryBound(to: Int32.self)[index]) / 2_147_483_648.0
    }
    return 0
}

func whisperResampleToPcm16(_ samples: [Float], inputRate: Double) -> [Int16] {
    guard !samples.isEmpty else { return [] }
    let outputRate = 16_000.0
    let outputCount = max(1, Int((Double(samples.count) * outputRate / inputRate).rounded()))
    let ratio = inputRate / outputRate
    return (0..<outputCount).map { index in
        let position = Double(index) * ratio
        let lower = min(samples.count - 1, Int(position.rounded(.down)))
        let upper = min(samples.count - 1, lower + 1)
        let fraction = Float(position - Double(lower))
        let value = samples[lower] * (1 - fraction) + samples[upper] * fraction
        return Int16(max(Double(Int16.min), min(Double(Int16.max), Double(value) * 32767.0)))
    }
}

func writeWhisperPcm16Wav(samples: [Int16], to url: URL) throws {
    var data = Data()
    func appendString(_ value: String) { data.append(value.data(using: .ascii)!) }
    func appendUInt32(_ value: UInt32) {
        var little = value.littleEndian
        data.append(Data(bytes: &little, count: 4))
    }
    func appendUInt16(_ value: UInt16) {
        var little = value.littleEndian
        data.append(Data(bytes: &little, count: 2))
    }
    let byteCount = UInt32(samples.count * 2)
    appendString("RIFF")
    appendUInt32(36 + byteCount)
    appendString("WAVEfmt ")
    appendUInt32(16)
    appendUInt16(1)
    appendUInt16(1)
    appendUInt32(16_000)
    appendUInt32(16_000 * 2)
    appendUInt16(2)
    appendUInt16(16)
    appendString("data")
    appendUInt32(byteCount)
    samples.withUnsafeBufferPointer { pointer in
        if let base = pointer.baseAddress {
            data.append(Data(bytes: base, count: samples.count * 2))
        }
    }
    try data.write(to: url, options: .atomic)
}

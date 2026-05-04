import AVFoundation
import AppKit
import CoreMedia
import CoreGraphics
import Foundation
import ScreenCaptureKit
import Speech

struct TranscriptLine: Codable {
    let kind: String
    let text: String
    let isFinal: Bool
    let confidence: Double
    let language: String
    let source: String
    let startedAtMs: Int64
    let endedAtMs: Int64
}

struct HealthLine: Codable {
    let kind: String
    let providerId: String
    let ready: Bool
    let supportsStreaming: Bool
    let supportsDiarization: Bool
    let supportsSourceHints: Bool
    let lastError: String?
}

struct CliOptions {
    var mode = "stream"
    var language = "zh-TW"
    var source = "mic"
    var durationSeconds: Double = 0
}

func parseOptions(_ args: [String]) -> CliOptions {
    var options = CliOptions()
    var index = 0
    while index < args.count {
        let arg = args[index]
        switch arg {
        case "--health":
            options.mode = "health"
        case "--request-screen-capture":
            options.mode = "request-screen-capture"
        case "--language":
            if index + 1 < args.count {
                options.language = args[index + 1]
                index += 1
            }
        case "--source":
            if index + 1 < args.count {
                options.source = args[index + 1]
                index += 1
            }
        case "--duration-seconds":
            if index + 1 < args.count {
                options.durationSeconds = Double(args[index + 1]) ?? 0
                index += 1
            }
        default:
            break
        }
        index += 1
    }
    return options
}

func writeJson<T: Encodable>(_ value: T) {
    let encoder = JSONEncoder()
    guard let data = try? encoder.encode(value), let line = String(data: data, encoding: .utf8) else {
        return
    }
    print(line)
    fflush(stdout)
}

func requestSpeechAuthorization() -> SFSpeechRecognizerAuthorizationStatus {
    let semaphore = DispatchSemaphore(value: 0)
    var status = SFSpeechRecognizerAuthorizationStatus.notDetermined
    SFSpeechRecognizer.requestAuthorization { newStatus in
        status = newStatus
        semaphore.signal()
    }
    semaphore.wait()
    return status
}

func hasScreenCaptureAccess() -> Bool {
    if #available(macOS 10.15, *) {
        return CGPreflightScreenCaptureAccess()
    }
    return true
}

func requestScreenCaptureAccessIfNeeded() -> Bool {
    if hasScreenCaptureAccess() {
        return true
    }
    if #available(macOS 10.15, *) {
        return CGRequestScreenCaptureAccess()
    }
    return true
}

func runScreenCaptureAccessRequestApp() -> Never {
    if hasScreenCaptureAccess() {
        exit(0)
    }
    if #available(macOS 10.15, *) {
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)
        app.activate(ignoringOtherApps: true)
        DispatchQueue.main.async {
            let granted = CGRequestScreenCaptureAccess()
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
                exit(granted ? 0 : 7)
            }
        }
        app.run()
    }
    exit(0)
}

final class MicSpeechStreamer {
    private let options: CliOptions
    private let recognizer: SFSpeechRecognizer
    private let audioEngine = AVAudioEngine()
    private let startedAt = Date()
    private var recognitionTask: SFSpeechRecognitionTask?
    private let done = DispatchSemaphore(value: 0)
    private var lastText = ""

    init?(options: CliOptions) {
        self.options = options
        guard let recognizer = SFSpeechRecognizer(locale: Locale(identifier: options.language)) else {
            return nil
        }
        self.recognizer = recognizer
    }

    func start() throws {
        guard options.source == "mic" else {
            throw NSError(domain: "MeetingCopilotSpeech", code: 3, userInfo: [NSLocalizedDescriptionKey: "system audio uses ScreenCaptureKit and is not handled by this mic streamer"])
        }
        guard recognizer.isAvailable else {
            throw NSError(domain: "MeetingCopilotSpeech", code: 4, userInfo: [NSLocalizedDescriptionKey: "speech recognizer is not available"])
        }

        let request = SFSpeechAudioBufferRecognitionRequest()
        request.shouldReportPartialResults = true
        request.addsPunctuation = true

        let inputNode = audioEngine.inputNode
        let format = inputNode.outputFormat(forBus: 0)
        inputNode.removeTap(onBus: 0)
        inputNode.installTap(onBus: 0, bufferSize: 1024, format: format) { buffer, _ in
            request.append(buffer)
        }

        recognitionTask = recognizer.recognitionTask(with: request) { [weak self] result, error in
            guard let self else { return }
            if let result {
                self.emit(result: result)
                if result.isFinal {
                    self.done.signal()
                }
            }
            if error != nil {
                self.done.signal()
            }
        }

        audioEngine.prepare()
        try audioEngine.start()

        if options.durationSeconds > 0 {
            DispatchQueue.global().asyncAfter(deadline: .now() + options.durationSeconds) { [weak self] in
                self?.stop()
            }
        }
        done.wait()
        stop()
    }

    private func emit(result: SFSpeechRecognitionResult) {
        let text = result.bestTranscription.formattedString.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, text != lastText else { return }
        lastText = text
        let endedAt = Int64(Date().timeIntervalSince(startedAt) * 1000)
        let startedAtMs = max(0, endedAt - 3000)
        let confidenceValues = result.bestTranscription.segments.map { Double($0.confidence) }.filter { $0 > 0 }
        let confidence = confidenceValues.isEmpty ? 0.55 : confidenceValues.reduce(0, +) / Double(confidenceValues.count)
        writeJson(TranscriptLine(
            kind: "transcript",
            text: text,
            isFinal: result.isFinal,
            confidence: confidence,
            language: options.language,
            source: options.source,
            startedAtMs: startedAtMs,
            endedAtMs: endedAt
        ))
    }

    func stop() {
        if audioEngine.isRunning {
            audioEngine.stop()
        }
        audioEngine.inputNode.removeTap(onBus: 0)
        recognitionTask?.finish()
        done.signal()
    }
}

@available(macOS 13.0, *)
final class SystemAudioSpeechStreamer: NSObject, SCStreamDelegate, SCStreamOutput {
    private let options: CliOptions
    private let recognizer: SFSpeechRecognizer
    private let startedAt = Date()
    private let done = DispatchSemaphore(value: 0)
    private var recognitionTask: SFSpeechRecognitionTask?
    private var recognitionRequest: SFSpeechAudioBufferRecognitionRequest?
    private var stream: SCStream?
    private var lastText = ""
    private var startupError: Error?

    init?(options: CliOptions) {
        self.options = options
        guard let recognizer = SFSpeechRecognizer(locale: Locale(identifier: options.language)) else {
            return nil
        }
        self.recognizer = recognizer
        super.init()
    }

    func start() throws {
        guard recognizer.isAvailable else {
            throw NSError(domain: "MeetingCopilotSpeech", code: 5, userInfo: [NSLocalizedDescriptionKey: "speech recognizer is not available"])
        }
        let request = SFSpeechAudioBufferRecognitionRequest()
        request.shouldReportPartialResults = true
        request.addsPunctuation = true
        recognitionRequest = request
        recognitionTask = recognizer.recognitionTask(with: request) { [weak self] result, error in
            guard let self else { return }
            if let result {
                self.emit(result: result)
                if result.isFinal {
                    self.done.signal()
                }
            }
            if error != nil {
                self.done.signal()
            }
        }

        Task {
            do {
                try await self.startCapture()
            } catch {
                self.startupError = error
                self.done.signal()
            }
        }

        if options.durationSeconds > 0 {
            DispatchQueue.global().asyncAfter(deadline: .now() + options.durationSeconds) { [weak self] in
                self?.stop()
            }
        }
        done.wait()
        stop()
        if let startupError {
            throw startupError
        }
    }

    private func startCapture() async throws {
        guard requestScreenCaptureAccessIfNeeded() else {
            throw NSError(domain: "MeetingCopilotSpeech", code: 7, userInfo: [NSLocalizedDescriptionKey: "screen capture permission is required for system audio; grant Screen Recording / Screen & System Audio Recording to Meeting Copilot and restart Meeting Copilot"])
        }
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
        guard let display = content.displays.first else {
            throw NSError(domain: "MeetingCopilotSpeech", code: 6, userInfo: [NSLocalizedDescriptionKey: "no display available for ScreenCaptureKit audio"])
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
        try stream.addStreamOutput(self, type: .audio, sampleHandlerQueue: DispatchQueue(label: "meeting-copilot.system-audio"))
        self.stream = stream
        try await stream.startCapture()
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .audio, sampleBuffer.isValid else { return }
        recognitionRequest?.appendAudioSampleBuffer(sampleBuffer)
    }

    private func emit(result: SFSpeechRecognitionResult) {
        let text = result.bestTranscription.formattedString.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, text != lastText else { return }
        lastText = text
        let endedAt = Int64(Date().timeIntervalSince(startedAt) * 1000)
        let startedAtMs = max(0, endedAt - 3000)
        let confidenceValues = result.bestTranscription.segments.map { Double($0.confidence) }.filter { $0 > 0 }
        let confidence = confidenceValues.isEmpty ? 0.55 : confidenceValues.reduce(0, +) / Double(confidenceValues.count)
        writeJson(TranscriptLine(
            kind: "transcript",
            text: text,
            isFinal: result.isFinal,
            confidence: confidence,
            language: options.language,
            source: options.source,
            startedAtMs: startedAtMs,
            endedAtMs: endedAt
        ))
    }

    func stop() {
        recognitionRequest?.endAudio()
        recognitionTask?.finish()
        if let stream {
            Task {
                try? await stream.stopCapture()
            }
        }
        done.signal()
    }
}

let options = parseOptions(Array(CommandLine.arguments.dropFirst()))
let recognizerAvailable = SFSpeechRecognizer(locale: Locale(identifier: options.language))?.isAvailable ?? false

if options.mode == "request-screen-capture" {
    runScreenCaptureAccessRequestApp()
}

if options.mode == "health" {
    let status = requestSpeechAuthorization()
    let needsScreenCapture = options.source == "system"
    let screenCaptureReady = !needsScreenCapture || hasScreenCaptureAccess()
    let ready = status == .authorized && recognizerAvailable && screenCaptureReady
    let lastError = ready ? nil : "speech authorization is \(status.rawValue), recognizerAvailable=\(recognizerAvailable), screenCaptureReady=\(screenCaptureReady)"
    writeJson(HealthLine(
        kind: "health",
        providerId: "macos-speech-native",
        ready: ready,
        supportsStreaming: true,
        supportsDiarization: false,
        supportsSourceHints: true,
        lastError: lastError
    ))
    exit(ready ? 0 : 2)
}

let status = requestSpeechAuthorization()
guard status == .authorized else {
    fputs("speech authorization denied or unavailable: \(status.rawValue)\n", stderr)
    exit(2)
}

do {
    if options.source == "system" {
        if #available(macOS 13.0, *) {
            guard let streamer = SystemAudioSpeechStreamer(options: options) else {
                fputs("failed to create system audio speech recognizer for \(options.language)\n", stderr)
                exit(3)
            }
            try streamer.start()
        } else {
            fputs("system audio capture requires macOS 13 or newer\n", stderr)
            exit(5)
        }
    } else {
        guard let streamer = MicSpeechStreamer(options: options) else {
            fputs("failed to create speech recognizer for \(options.language)\n", stderr)
            exit(3)
        }
        try streamer.start()
    }
} catch {
    fputs("\(error.localizedDescription)\n", stderr)
    exit(4)
}

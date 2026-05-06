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

struct WhisperJob: Codable {
    let file: String
    let language: String
    let source: String
    let route: String
    let startedAtMs: Int64
    let endedAtMs: Int64
}

struct CliOptions {
    var mode = "stream"
    var engine = "system-speech"
    var language = "zh-TW"
    var source = "mic"
    var durationSeconds: Double = 0
    var whisperRunner: String?
    var whisperModel: String?
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
        case "--engine":
            if index + 1 < args.count {
                options.engine = args[index + 1]
                index += 1
            }
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
        case "--whisper-runner":
            if index + 1 < args.count {
                options.whisperRunner = args[index + 1]
                index += 1
            }
        case "--whisper-model":
            if index + 1 < args.count {
                options.whisperModel = args[index + 1]
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

final class WhisperChunkTranscriber {
    private let options: CliOptions
    private let runner: String
    private let model: String
    private let startedAt = Date()
    private let queue = DispatchQueue(label: "meeting-copilot.whisper-chunks")
    private var samples: [Int16] = []
    private var chunkStartedAtMs: Int64 = 0
    private var chunkIndex = 0
    private var isClosed = false
    private var process: Process?
    private var stdinPipe: Pipe?
    private let sampleRate = 16_000
    private let chunkSampleCount = 16_000 * 3
    private let minimumFlushSampleCount = 4_000
    private let instanceId = UUID().uuidString

    init(options: CliOptions) throws {
        guard let runner = options.whisperRunner, !runner.isEmpty else {
            throw NSError(domain: "MeetingCopilotSpeech", code: 20, userInfo: [NSLocalizedDescriptionKey: "missing --whisper-runner"])
        }
        guard let model = options.whisperModel, !model.isEmpty else {
            throw NSError(domain: "MeetingCopilotSpeech", code: 21, userInfo: [NSLocalizedDescriptionKey: "missing --whisper-model"])
        }
        self.options = options
        self.runner = runner
        self.model = model
        try startRunner()
    }

    deinit {
        closeRunner()
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
        forwardPipe(stdoutPipe, toStdout: true)
        forwardPipe(stderrPipe, toStdout: false)
    }

    private func closeRunner(graceful: Bool = false) {
        try? stdinPipe?.fileHandleForWriting.close()
        if let process, process.isRunning {
            if graceful {
                process.waitUntilExit()
            } else {
                process.terminate()
            }
        }
    }

    private func forwardPipe(_ pipe: Pipe, toStdout: Bool) {
        DispatchQueue.global(qos: .utility).async {
            while true {
                let data = pipe.fileHandleForReading.availableData
                if data.isEmpty { break }
                guard let text = String(data: data, encoding: .utf8), !text.isEmpty else { continue }
                if toStdout {
                    fputs(text, stdout)
                    fflush(stdout)
                } else {
                    fputs(text, stderr)
                }
            }
        }
    }

    func append(_ buffer: AVAudioPCMBuffer) {
        guard let converted = convertAudioPCMBufferToPcm16(buffer) else { return }
        append(samples: converted)
    }

    func append(_ sampleBuffer: CMSampleBuffer) {
        guard let converted = convertSampleBufferToPcm16(sampleBuffer) else { return }
        append(samples: converted)
    }

    private func append(samples newSamples: [Int16]) {
        guard !newSamples.isEmpty else { return }
        queue.async { [weak self] in
            guard let self, !self.isClosed else { return }
            if self.samples.isEmpty {
                self.chunkStartedAtMs = self.elapsedMs()
            }
            self.samples.append(contentsOf: newSamples)
            while self.samples.count >= self.chunkSampleCount {
                let chunk = Array(self.samples.prefix(self.chunkSampleCount))
                self.samples.removeFirst(self.chunkSampleCount)
                let started = self.chunkStartedAtMs
                let ended = started + Int64(Double(chunk.count) / Double(self.sampleRate) * 1000)
                self.chunkStartedAtMs = ended
                self.transcribe(chunk: chunk, startedAtMs: started, endedAtMs: ended)
            }
        }
    }

    func finish() {
        queue.sync {
            isClosed = true
            if samples.count >= minimumFlushSampleCount {
                let chunk = samples
                let started = chunkStartedAtMs
                let ended = started + Int64(Double(chunk.count) / Double(sampleRate) * 1000)
                samples.removeAll()
                transcribe(chunk: chunk, startedAtMs: started, endedAtMs: ended)
            }
            closeRunner(graceful: true)
        }
    }

    private func elapsedMs() -> Int64 {
        Int64(Date().timeIntervalSince(startedAt) * 1000)
    }

    private func transcribe(chunk: [Int16], startedAtMs: Int64, endedAtMs: Int64) {
        chunkIndex += 1
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("meeting-copilot-whisper-\(instanceId)-\(options.source)-\(chunkIndex).wav")
        do {
            try writePcm16Wav(samples: chunk, sampleRate: sampleRate, to: url)
            let job = WhisperJob(
                file: url.path,
                language: options.language,
                source: options.source,
                route: options.source,
                startedAtMs: startedAtMs,
                endedAtMs: endedAtMs
            )
            let data = try JSONEncoder().encode(job)
            guard let input = stdinPipe?.fileHandleForWriting else {
                throw NSError(domain: "MeetingCopilotSpeech", code: 25, userInfo: [NSLocalizedDescriptionKey: "Whisper runner stdin is unavailable"])
            }
            input.write(data)
            input.write(Data("\n".utf8))
        } catch {
            fputs("Whisper chunk transcription failed: \(error.localizedDescription)\n", stderr)
        }
    }
}

final class MicWhisperStreamer {
    private let options: CliOptions
    private let audioEngine = AVAudioEngine()
    private let transcriber: WhisperChunkTranscriber
    private let done = DispatchSemaphore(value: 0)

    init(options: CliOptions) throws {
        self.options = options
        self.transcriber = try WhisperChunkTranscriber(options: options)
    }

    func start() throws {
        guard options.source == "mic" else {
            throw NSError(domain: "MeetingCopilotSpeech", code: 22, userInfo: [NSLocalizedDescriptionKey: "MicWhisperStreamer only supports mic"])
        }
        let inputNode = audioEngine.inputNode
        let format = inputNode.outputFormat(forBus: 0)
        inputNode.removeTap(onBus: 0)
        inputNode.installTap(onBus: 0, bufferSize: 4096, format: format) { [weak self] buffer, _ in
            self?.transcriber.append(buffer)
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

    func stop() {
        if audioEngine.isRunning {
            audioEngine.stop()
        }
        audioEngine.inputNode.removeTap(onBus: 0)
        transcriber.finish()
        done.signal()
    }
}

@available(macOS 13.0, *)
final class SystemAudioWhisperStreamer: NSObject, SCStreamDelegate, SCStreamOutput {
    private let options: CliOptions
    private let transcriber: WhisperChunkTranscriber
    private let done = DispatchSemaphore(value: 0)
    private var stream: SCStream?
    private var startupError: Error?

    init(options: CliOptions) throws {
        self.options = options
        self.transcriber = try WhisperChunkTranscriber(options: options)
        super.init()
    }

    func start() throws {
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
            throw NSError(domain: "MeetingCopilotSpeech", code: 23, userInfo: [NSLocalizedDescriptionKey: "screen capture permission is required for local Whisper system audio"])
        }
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
        guard let display = content.displays.first else {
            throw NSError(domain: "MeetingCopilotSpeech", code: 24, userInfo: [NSLocalizedDescriptionKey: "no display available for ScreenCaptureKit audio"])
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
        try stream.addStreamOutput(self, type: .audio, sampleHandlerQueue: DispatchQueue(label: "meeting-copilot.whisper-system-audio"))
        self.stream = stream
        try await stream.startCapture()
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .audio, sampleBuffer.isValid else { return }
        transcriber.append(sampleBuffer)
    }

    func stop() {
        if let stream {
            Task {
                try? await stream.stopCapture()
            }
        }
        transcriber.finish()
        done.signal()
    }
}

func convertAudioPCMBufferToPcm16(_ buffer: AVAudioPCMBuffer) -> [Int16]? {
    let channelCount = max(1, Int(buffer.format.channelCount))
    let inputRate = buffer.format.sampleRate
    let frameCount = Int(buffer.frameLength)
    guard frameCount > 0 else { return [] }
    var mono = [Float]()
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
    return resampleToPcm16(mono, inputRate: inputRate)
}

func convertSampleBufferToPcm16(_ sampleBuffer: CMSampleBuffer) -> [Int16]? {
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
                sample += sampleFromAudioData(data, index: frame * channelCount + channel, asbd: asbd)
            }
        } else {
            for channel in 0..<min(channelCount, buffers.count) {
                guard let data = buffers[channel].mData else { continue }
                sample += sampleFromAudioData(data, index: frame, asbd: asbd)
            }
        }
        mono.append(sample / Float(channelCount))
    }
    return resampleToPcm16(mono, inputRate: asbd.mSampleRate)
}

func sampleFromAudioData(_ data: UnsafeMutableRawPointer, index: Int, asbd: AudioStreamBasicDescription) -> Float {
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

func resampleToPcm16(_ samples: [Float], inputRate: Double) -> [Int16] {
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

func writePcm16Wav(samples: [Int16], sampleRate: Int, to url: URL) throws {
    var data = Data()
    func appendString(_ value: String) {
        data.append(value.data(using: .ascii)!)
    }
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
    appendUInt32(UInt32(sampleRate))
    appendUInt32(UInt32(sampleRate * 2))
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

if options.engine == "whisper" {
    do {
        if options.source == "system" {
            if #available(macOS 13.0, *) {
                try SystemAudioWhisperStreamer(options: options).start()
            } else {
                fputs("system audio capture requires macOS 13 or newer\n", stderr)
                exit(5)
            }
        } else {
            try MicWhisperStreamer(options: options).start()
        }
    } catch {
        fputs("\(error.localizedDescription)\n", stderr)
        exit(4)
    }
    exit(0)
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

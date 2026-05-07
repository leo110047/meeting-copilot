use std::env;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::thread;

fn main() {
    let args: Vec<String> = env::args().collect();
    let language = sanitize_language(arg_value(&args, "--language").unwrap_or_else(|| "zh-TW".to_string()));
    let source = sanitize_source(arg_value(&args, "--source").unwrap_or_else(|| "mic".to_string()));
    let engine = arg_value(&args, "--engine").unwrap_or_else(|| "system-speech".to_string());
    let script = if engine == "whisper" {
        let runner = arg_value(&args, "--whisper-runner").unwrap_or_default();
        let model = arg_value(&args, "--whisper-model").unwrap_or_default();
        let stop_file = arg_value(&args, "--stop-file").unwrap_or_default();
        if runner.is_empty() || model.is_empty() {
            eprintln!("missing --whisper-runner or --whisper-model");
            std::process::exit(2);
        }
        whisper_powershell_script(&language, &source, &runner, &model, &stop_file)
    } else if source == "system" {
        system_loopback_powershell_script(&language)
    } else {
        microphone_powershell_script(&language)
    };
    let mut child = match Command::new("powershell.exe")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            eprintln!("failed to start Windows SpeechRecognition bridge: {error}");
            std::process::exit(1);
        }
    };
    let stderr = child.stderr.take().expect("speech bridge stderr");
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            eprintln!("{line}");
        }
    });
    let stdout = child.stdout.take().expect("speech bridge stdout");
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        println!("{line}");
    }
    let status = child.wait().expect("speech bridge wait");
    std::process::exit(status.code().unwrap_or(1));
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn sanitize_language(value: String) -> String {
    match value.as_str() {
        "zh-TW" | "zh-CN" | "en-US" | "en-GB" | "ja-JP" | "ko-KR" => value,
        _ => "zh-TW".to_string(),
    }
}

fn sanitize_source(value: String) -> String {
    match value.as_str() {
        "mic" | "system" | "mixed" => value,
        _ => "mic".to_string(),
    }
}

fn ps_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn whisper_powershell_script(
    language: &str,
    source: &str,
    runner: &str,
    model: &str,
    stop_file: &str,
) -> String {
    let data_flow = if source == "system" { "eRender" } else { "eCapture" };
    let loopback = if source == "system" { "true" } else { "false" };
    format!(
        r#"
Add-Type -Language CSharp -ReferencedAssemblies @('System.dll','System.Core.dll') -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Collections.Concurrent;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Threading;

namespace MeetingCopilot {{
  public enum EDataFlow {{ eRender = 0, eCapture = 1, eAll = 2 }}
  public enum ERole {{ eConsole = 0, eMultimedia = 1, eCommunications = 2 }}

  [Flags]
  public enum CLSCTX : uint {{ InprocServer = 0x1, InprocHandler = 0x2, LocalServer = 0x4, All = 0x17 }}

  [StructLayout(LayoutKind.Sequential)]
  public struct WaveFormatEx {{
    public ushort wFormatTag;
    public ushort nChannels;
    public uint nSamplesPerSec;
    public uint nAvgBytesPerSec;
    public ushort nBlockAlign;
    public ushort wBitsPerSample;
    public ushort cbSize;
  }}

  [ComImport]
  [Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")]
  public class MMDeviceEnumeratorComObject {{ }}

  [ComImport]
  [Guid("A95664D2-9614-4F35-A746-DE8DB63617E6")]
  [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  public interface IMMDeviceEnumerator {{
    int EnumAudioEndpoints(EDataFlow dataFlow, uint stateMask, out IntPtr devices);
    int GetDefaultAudioEndpoint(EDataFlow dataFlow, ERole role, out IMMDevice endpoint);
    int GetDevice([MarshalAs(UnmanagedType.LPWStr)] string id, out IMMDevice device);
    int RegisterEndpointNotificationCallback(IntPtr client);
    int UnregisterEndpointNotificationCallback(IntPtr client);
  }}

  [ComImport]
  [Guid("D666063F-1587-4E43-81F1-B948E807363F")]
  [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  public interface IMMDevice {{
    int Activate(ref Guid iid, CLSCTX clsCtx, IntPtr activationParams, [MarshalAs(UnmanagedType.IUnknown)] out object interfacePointer);
    int OpenPropertyStore(int stgmAccess, out IntPtr properties);
    int GetId([MarshalAs(UnmanagedType.LPWStr)] out string id);
    int GetState(out int state);
  }}

  [ComImport]
  [Guid("1CB9AD4C-DBFA-4c32-B178-C2F568A703B2")]
  [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  public interface IAudioClient {{
    int Initialize(int shareMode, int streamFlags, long bufferDuration, long periodicity, IntPtr waveFormat, IntPtr audioSessionGuid);
    int GetBufferSize(out uint bufferSize);
    int GetStreamLatency(out long latency);
    int GetCurrentPadding(out uint padding);
    int IsFormatSupported(int shareMode, IntPtr waveFormat, IntPtr closestMatch);
    int GetMixFormat(out IntPtr waveFormat);
    int GetDevicePeriod(out long defaultPeriod, out long minimumPeriod);
    int Start();
    int Stop();
    int Reset();
    int SetEventHandle(IntPtr eventHandle);
    int GetService(ref Guid iid, [MarshalAs(UnmanagedType.IUnknown)] out object service);
  }}

  [ComImport]
  [Guid("C8ADBD64-E71E-48a0-A4DE-185C395CD317")]
  [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  public interface IAudioCaptureClient {{
    int GetBuffer(out IntPtr data, out uint frames, out uint flags, out ulong devicePosition, out ulong qpcPosition);
    int ReleaseBuffer(uint frames);
    int GetNextPacketSize(out uint frames);
  }}

  public sealed class BlockingAudioStream : Stream {{
    private readonly BlockingCollection<byte[]> queue = new BlockingCollection<byte[]>(128);
    private byte[] current = Array.Empty<byte>();
    private int offset = 0;
    public bool Completed {{ get {{ return queue.IsCompleted && offset >= current.Length; }} }}
    public override bool CanRead {{ get {{ return true; }} }}
    public override bool CanSeek {{ get {{ return false; }} }}
    public override bool CanWrite {{ get {{ return true; }} }}
    public override long Length {{ get {{ throw new NotSupportedException(); }} }}
    public override long Position {{ get {{ throw new NotSupportedException(); }} set {{ throw new NotSupportedException(); }} }}
    public void Complete() {{ queue.CompleteAdding(); }}
    public override void Flush() {{ }}
    public override long Seek(long offset, SeekOrigin origin) {{ throw new NotSupportedException(); }}
    public override void SetLength(long value) {{ throw new NotSupportedException(); }}
    public override void Write(byte[] buffer, int offset, int count) {{
      var copy = new byte[count];
      Buffer.BlockCopy(buffer, offset, copy, 0, count);
      if (!queue.IsAddingCompleted) queue.Add(copy);
    }}
    public override int Read(byte[] buffer, int offset, int count) {{
      var written = 0;
      while (written == 0) {{
        if (this.offset >= current.Length) {{
          byte[] next;
          if (!queue.TryTake(out next, 100)) {{
            if (queue.IsCompleted) return 0;
            return 0;
          }}
          current = next;
          this.offset = 0;
        }}
        var available = Math.Min(count - written, current.Length - this.offset);
        Buffer.BlockCopy(current, this.offset, buffer, offset + written, available);
        this.offset += available;
        written += available;
      }}
      return written;
    }}
  }}

  public sealed class WasapiPcmCapture {{
    private const int AUDCLNT_SHAREMODE_SHARED = 0;
    private const int AUDCLNT_STREAMFLAGS_LOOPBACK = 0x00020000;
    private const uint AUDCLNT_BUFFERFLAGS_SILENT = 0x00000002;
    private const ushort WAVE_FORMAT_PCM = 1;
    private const ushort WAVE_FORMAT_IEEE_FLOAT = 3;
    private readonly BlockingAudioStream stream;
    private readonly EDataFlow dataFlow;
    private readonly bool loopback;
    private bool running;

    public WasapiPcmCapture(BlockingAudioStream stream, EDataFlow dataFlow, bool loopback) {{
      this.stream = stream;
      this.dataFlow = dataFlow;
      this.loopback = loopback;
    }}

    public void Start() {{
      running = true;
      var thread = new Thread(CaptureLoop);
      thread.IsBackground = true;
      thread.Start();
    }}

    public void Stop() {{
      running = false;
      stream.Complete();
    }}

    private void CaptureLoop() {{
      IAudioClient audioClient = null;
      try {{
        var enumerator = (IMMDeviceEnumerator)new MMDeviceEnumeratorComObject();
        IMMDevice device;
        Check(enumerator.GetDefaultAudioEndpoint(dataFlow, ERole.eMultimedia, out device), "GetDefaultAudioEndpoint");
        var iidAudioClient = new Guid("1CB9AD4C-DBFA-4c32-B178-C2F568A703B2");
        object audioClientObject;
        Check(device.Activate(ref iidAudioClient, CLSCTX.All, IntPtr.Zero, out audioClientObject), "IMMDevice.Activate(IAudioClient)");
        audioClient = (IAudioClient)audioClientObject;
        IntPtr mixFormatPtr;
        Check(audioClient.GetMixFormat(out mixFormatPtr), "IAudioClient.GetMixFormat");
        var mixFormat = (WaveFormatEx)Marshal.PtrToStructure(mixFormatPtr, typeof(WaveFormatEx));
        var flags = loopback ? AUDCLNT_STREAMFLAGS_LOOPBACK : 0;
        Check(audioClient.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 10000000, 0, mixFormatPtr, IntPtr.Zero), "IAudioClient.Initialize");
        var iidCaptureClient = new Guid("C8ADBD64-E71E-48a0-A4DE-185C395CD317");
        object captureClientObject;
        Check(audioClient.GetService(ref iidCaptureClient, out captureClientObject), "IAudioClient.GetService(IAudioCaptureClient)");
        var captureClient = (IAudioCaptureClient)captureClientObject;
        Check(audioClient.Start(), "IAudioClient.Start");
        Console.Error.WriteLine("Windows WASAPI Whisper capture started: " + dataFlow + " " + mixFormat.nSamplesPerSec + "Hz/" + mixFormat.nChannels + "ch/" + mixFormat.wBitsPerSample + "bit/tag" + mixFormat.wFormatTag);
        while (running) {{
          uint packetFrames;
          Check(captureClient.GetNextPacketSize(out packetFrames), "IAudioCaptureClient.GetNextPacketSize");
          if (packetFrames == 0) {{
            Thread.Sleep(10);
            continue;
          }}
          while (packetFrames > 0) {{
            IntPtr data;
            uint frames;
            uint flagsOut;
            ulong devicePosition;
            ulong qpcPosition;
            Check(captureClient.GetBuffer(out data, out frames, out flagsOut, out devicePosition, out qpcPosition), "IAudioCaptureClient.GetBuffer");
            var pcm16 = ConvertTo16kMonoPcm(data, frames, flagsOut, mixFormat);
            if (pcm16.Length > 0) stream.Write(pcm16, 0, pcm16.Length);
            Check(captureClient.ReleaseBuffer(frames), "IAudioCaptureClient.ReleaseBuffer");
            Check(captureClient.GetNextPacketSize(out packetFrames), "IAudioCaptureClient.GetNextPacketSize");
          }}
        }}
      }} catch (Exception error) {{
        Console.Error.WriteLine("Windows WASAPI Whisper capture failed: " + error);
        stream.Complete();
      }} finally {{
        if (audioClient != null) audioClient.Stop();
      }}
    }}

    private static byte[] ConvertTo16kMonoPcm(IntPtr data, uint frames, uint flags, WaveFormatEx format) {{
      if (frames == 0 || data == IntPtr.Zero) return Array.Empty<byte>();
      var inputRate = (int)format.nSamplesPerSec;
      var channels = Math.Max(1, (int)format.nChannels);
      var outputFrames = Math.Max(1, (int)Math.Round(frames * 16000.0 / inputRate));
      var output = new byte[outputFrames * 2];
      for (int i = 0; i < outputFrames; i++) {{
        var sourceFrame = Math.Min((int)frames - 1, (int)Math.Round(i * inputRate / 16000.0));
        var mono = 0.0;
        if ((flags & AUDCLNT_BUFFERFLAGS_SILENT) == 0) {{
          for (int ch = 0; ch < channels; ch++) mono += SampleAt(data, sourceFrame, ch, channels, format);
          mono /= channels;
        }}
        var sample = (short)Math.Max(short.MinValue, Math.Min(short.MaxValue, Math.Round(mono * 32767.0)));
        output[i * 2] = (byte)(sample & 0xff);
        output[i * 2 + 1] = (byte)((sample >> 8) & 0xff);
      }}
      return output;
    }}

    private static double SampleAt(IntPtr data, int frame, int channel, int channels, WaveFormatEx format) {{
      var index = frame * channels + channel;
      if (format.wFormatTag == WAVE_FORMAT_IEEE_FLOAT && format.wBitsPerSample == 32) {{
        return Math.Max(-1.0, Math.Min(1.0, Marshal.PtrToStructure<float>(IntPtr.Add(data, index * 4))));
      }}
      if (format.wFormatTag == WAVE_FORMAT_PCM && format.wBitsPerSample == 16) {{
        return Marshal.PtrToStructure<short>(IntPtr.Add(data, index * 2)) / 32768.0;
      }}
      if (format.wBitsPerSample == 32) {{
        return Math.Max(-1.0, Math.Min(1.0, Marshal.PtrToStructure<float>(IntPtr.Add(data, index * 4))));
      }}
      if (format.wBitsPerSample == 16) {{
        return Marshal.PtrToStructure<short>(IntPtr.Add(data, index * 2)) / 32768.0;
      }}
      throw new NotSupportedException("unsupported Windows mix format: tag=" + format.wFormatTag + " bits=" + format.wBitsPerSample);
    }}

    private static void Check(int hr, string operation) {{
      if (hr < 0) Marshal.ThrowExceptionForHR(hr);
    }}
  }}

  public static class WhisperLoop {{
    private const int ChunkBytes = 16000 * 2 * 3;
    private const int MaxDeferredPostMeetingChunks = 1200;
    private const int PostMeetingMillisecondsPerChunk = 8000;
    private const int MinPostMeetingStopTimeoutMs = 30000;
    private const int MaxPostMeetingStopTimeoutMs = 1800000;

    public static void Run(string language, string source, string runner, string model, string stopFile, EDataFlow dataFlow, bool loopback) {{
      var lanes = source == "mixed"
        ? new List<CaptureLane> {{
            new CaptureLane("mic", EDataFlow.eCapture, false),
            new CaptureLane("system", EDataFlow.eRender, true)
          }}
        : new List<CaptureLane> {{ new CaptureLane(source, dataFlow, loopback) }};
      foreach (var lane in lanes) lane.Start();
      var whisper = new PersistentWhisperRunner(runner, model);
      var started = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
      var chunkIndex = 0;
      var postMeetingChunkIndex = 0;
      while (true) {{
        var stopRequested = !String.IsNullOrWhiteSpace(stopFile) && File.Exists(stopFile);
        foreach (var lane in lanes) {{
          var chunk = ReadChunk(lane.Stream, Math.Max(1, ChunkBytes - lane.Pending.Count));
          if (chunk.Length > 0) lane.Pending.AddRange(chunk);
          while (lane.Pending.Count >= ChunkBytes) {{
            var ready = lane.Pending.GetRange(0, ChunkBytes).ToArray();
            lane.Pending.RemoveRange(0, ChunkBytes);
            var chunkStarted = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() - started - (long)(ready.Length / 2.0 / 16000.0 * 1000.0);
            var chunkEnded = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() - started;
            if (source == "mixed" && lane.Source == "mic") {{
              if (whisper.CanDeferChunk()) {{
                postMeetingChunkIndex++;
                whisper.DeferChunk(language, lane.Source, ready, postMeetingChunkIndex, Math.Max(0, chunkStarted), Math.Max(0, chunkEnded));
              }} else {{
                whisper.ReportDeferredChunkDropped();
              }}
            }} else {{
              chunkIndex++;
              whisper.TranscribeChunk(language, lane.Source, ready, chunkIndex, Math.Max(0, chunkStarted), Math.Max(0, chunkEnded));
            }}
          }}
        }}
        if (!stopRequested && !lanes.All(lane => lane.Completed)) {{
          Thread.Sleep(50);
        }}
        if (stopRequested || lanes.All(lane => lane.Completed)) {{
          foreach (var lane in lanes) {{
            if (lane.Pending.Count >= 16000 * 2 / 4) {{
              var ready = lane.Pending.ToArray();
              lane.Pending.Clear();
              var chunkStarted = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() - started - (long)(ready.Length / 2.0 / 16000.0 * 1000.0);
              var chunkEnded = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() - started;
              if (source == "mixed" && lane.Source == "mic") {{
                if (whisper.CanDeferChunk()) {{
                  postMeetingChunkIndex++;
                  whisper.DeferChunk(language, lane.Source, ready, postMeetingChunkIndex, Math.Max(0, chunkStarted), Math.Max(0, chunkEnded));
                }} else {{
                  whisper.ReportDeferredChunkDropped();
                }}
              }} else {{
                chunkIndex++;
                whisper.TranscribeChunk(language, lane.Source, ready, chunkIndex, Math.Max(0, chunkStarted), Math.Max(0, chunkEnded));
              }}
            }}
            lane.Stop();
          }}
          var postMeetingJobs = whisper.TranscribeDeferred();
          whisper.Close(postMeetingJobs);
          return;
        }}
      }}
    }}

    public sealed class CaptureLane {{
      public string Source {{ get; private set; }}
      public BlockingAudioStream Stream {{ get; private set; }}
      public List<byte> Pending {{ get; private set; }}
      private readonly WasapiPcmCapture capture;
      public bool Completed {{ get {{ return Stream.Completed; }} }}

      public CaptureLane(string source, EDataFlow dataFlow, bool loopback) {{
        Source = source;
        Stream = new BlockingAudioStream();
        Pending = new List<byte>(ChunkBytes);
        capture = new WasapiPcmCapture(Stream, dataFlow, loopback);
      }}

      public void Start() {{ capture.Start(); }}
      public void Stop() {{ capture.Stop(); }}
    }}

    private static byte[] ReadChunk(Stream stream, int targetBytes) {{
      var buffer = new byte[targetBytes];
      var offset = 0;
      while (offset < targetBytes) {{
        var read = stream.Read(buffer, offset, targetBytes - offset);
        if (read <= 0) break;
        offset += read;
      }}
      if (offset == buffer.Length) return buffer;
      var trimmed = new byte[offset];
      Buffer.BlockCopy(buffer, 0, trimmed, 0, offset);
      return trimmed;
    }}

    public sealed class PersistentWhisperRunner {{
      private readonly Process process;
      private readonly List<DeferredWhisperJob> deferred = new List<DeferredWhisperJob>();
      private bool deferredDropReported = false;

      public PersistentWhisperRunner(string runner, string model) {{
        SweepStalePostMeetingFiles();
        process = new Process();
        process.StartInfo.FileName = runner;
        process.StartInfo.UseShellExecute = false;
        process.StartInfo.RedirectStandardInput = true;
        process.StartInfo.RedirectStandardOutput = true;
        process.StartInfo.RedirectStandardError = true;
        process.StartInfo.Arguments = "--serve --model " + QuoteArgument(model);
        process.Start();
        Forward(process.StandardOutput, false);
        Forward(process.StandardError, true);
      }}

      public void TranscribeChunk(string language, string source, byte[] pcm, int index, long startedAtMs, long endedAtMs) {{
        var path = Path.Combine(Path.GetTempPath(), "meeting-copilot-whisper-" + Process.GetCurrentProcess().Id + "-" + source + "-" + index + ".wav");
        try {{
          WriteWav(path, pcm);
          SendJob(new DeferredWhisperJob(path, language, source, source, startedAtMs, endedAtMs));
        }} catch (Exception error) {{
          Console.Error.WriteLine("Whisper chunk transcription failed: " + error.Message);
        }}
      }}

      public bool CanDeferChunk() {{
        return deferred.Count < MaxDeferredPostMeetingChunks;
      }}

      public void ReportDeferredChunkDropped() {{
        if (!deferredDropReported) {{
          Console.Error.WriteLine("Whisper post-meeting chunk dropped: deferred mic cap reached");
          deferredDropReported = true;
        }}
      }}

      public void DeferChunk(string language, string source, byte[] pcm, int index, long startedAtMs, long endedAtMs) {{
        var path = Path.Combine(Path.GetTempPath(), "meeting-copilot-whisper-" + Process.GetCurrentProcess().Id + "-post-meeting-" + source + "-" + index + ".wav");
        try {{
          WriteWav(path, pcm);
          deferred.Add(new DeferredWhisperJob(path, language, source, "post_meeting_" + source, startedAtMs, endedAtMs));
        }} catch (Exception error) {{
          Console.Error.WriteLine("Whisper post-meeting chunk buffering failed: " + error.Message);
        }}
      }}

      public int TranscribeDeferred() {{
        var dispatched = 0;
        foreach (var job in deferred) {{
          try {{
            SendJob(job);
            dispatched++;
          }} catch (Exception error) {{
            try {{ File.Delete(job.Path); }} catch {{ }}
            Console.Error.WriteLine("Whisper post-meeting chunk transcription failed: " + error.Message);
          }}
        }}
        return dispatched;
      }}

      public void Close(int postMeetingJobCount) {{
        try {{ process.StandardInput.Close(); }} catch {{ }}
        var timeoutMs = postMeetingJobCount > 0
          ? Math.Min(MaxPostMeetingStopTimeoutMs, Math.Max(MinPostMeetingStopTimeoutMs, postMeetingJobCount * PostMeetingMillisecondsPerChunk))
          : 10000;
        if (!process.WaitForExit(timeoutMs)) {{
          try {{ process.Kill(); }} catch {{ }}
          process.WaitForExit();
        }}
        CleanupDeferredFiles();
        deferred.Clear();
      }}

      private void SendJob(DeferredWhisperJob job) {{
        process.StandardInput.WriteLine("{{\"file\":\"" + JsonEscape(job.Path) + "\",\"language\":\"" + JsonEscape(job.Language) + "\",\"source\":\"" + JsonEscape(job.Source) + "\",\"route\":\"" + JsonEscape(job.Route) + "\",\"startedAtMs\":" + job.StartedAtMs.ToString(CultureInfo.InvariantCulture) + ",\"endedAtMs\":" + job.EndedAtMs.ToString(CultureInfo.InvariantCulture) + "}}");
        process.StandardInput.Flush();
      }}

      private static void Forward(StreamReader reader, bool error) {{
        var thread = new Thread(delegate() {{
          string line;
          while ((line = reader.ReadLine()) != null) {{
            if (error) Console.Error.WriteLine(line);
            else Console.WriteLine(line);
          }}
        }});
        thread.IsBackground = true;
        thread.Start();
      }}

      private static void SweepStalePostMeetingFiles() {{
        try {{
          foreach (var path in Directory.GetFiles(Path.GetTempPath(), "meeting-copilot-whisper-*-post-meeting-*.wav")) {{
            // Six hours is intentionally wider than normal meetings so concurrent
            // in-flight post-meeting chunks from another app instance survive.
            if ((DateTime.UtcNow - File.GetLastWriteTimeUtc(path)).TotalHours < 6) continue;
            try {{ File.Delete(path); }} catch {{ }}
          }}
        }} catch {{ }}
      }}

      private void CleanupDeferredFiles() {{
        foreach (var job in deferred) {{
          try {{ File.Delete(job.Path); }} catch {{ }}
        }}
      }}

      private sealed class DeferredWhisperJob {{
        public string Path {{ get; private set; }}
        public string Language {{ get; private set; }}
        public string Source {{ get; private set; }}
        public string Route {{ get; private set; }}
        public long StartedAtMs {{ get; private set; }}
        public long EndedAtMs {{ get; private set; }}

        public DeferredWhisperJob(string path, string language, string source, string route, long startedAtMs, long endedAtMs) {{
          Path = path;
          Language = language;
          Source = source;
          Route = route;
          StartedAtMs = startedAtMs;
          EndedAtMs = endedAtMs;
        }}
      }}
    }}

    private static void WriteWav(string path, byte[] pcm) {{
      using (var writer = new BinaryWriter(File.Open(path, FileMode.Create, FileAccess.Write, FileShare.Read))) {{
        writer.Write(System.Text.Encoding.ASCII.GetBytes("RIFF"));
        writer.Write(36 + pcm.Length);
        writer.Write(System.Text.Encoding.ASCII.GetBytes("WAVEfmt "));
        writer.Write(16);
        writer.Write((short)1);
        writer.Write((short)1);
        writer.Write(16000);
        writer.Write(16000 * 2);
        writer.Write((short)2);
        writer.Write((short)16);
        writer.Write(System.Text.Encoding.ASCII.GetBytes("data"));
        writer.Write(pcm.Length);
        writer.Write(pcm);
      }}
    }}

    private static string QuoteArgument(string value) {{
      return "\"" + value.Replace("\\", "\\\\").Replace("\"", "\\\"") + "\"";
    }}

    private static string JsonEscape(string value) {{
      return value.Replace("\\", "\\\\").Replace("\"", "\\\"").Replace("\r", "\\r").Replace("\n", "\\n");
    }}
  }}
}}
'@

[MeetingCopilot.WhisperLoop]::Run({language}, {source}, {runner}, {model}, {stop_file}, [MeetingCopilot.EDataFlow]::{data_flow}, ${loopback})
"#,
        language = ps_single_quote(language),
        source = ps_single_quote(source),
        runner = ps_single_quote(runner),
        model = ps_single_quote(model),
        stop_file = ps_single_quote(stop_file),
        data_flow = data_flow,
        loopback = loopback
    )
}

fn microphone_powershell_script(language: &str) -> String {
    format!(
        r#"
Add-Type -AssemblyName System.Speech
$recognizer = New-Object System.Speech.Recognition.SpeechRecognitionEngine([Globalization.CultureInfo]::GetCultureInfo('{language}'))
$recognizer.SetInputToDefaultAudioDevice()
$recognizer.LoadGrammar((New-Object System.Speech.Recognition.DictationGrammar))
$started = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
while ($true) {{
  $result = $recognizer.Recognize()
  if ($null -ne $result -and $result.Text.Trim().Length -gt 0) {{
    $now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $payload = [ordered]@{{
      kind = 'transcript'
      text = $result.Text
      isFinal = $true
      confidence = [double]$result.Confidence
      language = '{language}'
      source = 'mic'
      startedAtMs = [int64]([Math]::Max(0, $now - $started - 3000))
      endedAtMs = [int64]([Math]::Max(0, $now - $started))
    }}
    $payload | ConvertTo-Json -Compress
    [Console]::Out.Flush()
  }}
}}
"#
    )
}

fn system_loopback_powershell_script(language: &str) -> String {
    format!(
        r#"
Add-Type -AssemblyName System.Speech
Add-Type -Language CSharp -ReferencedAssemblies @('System.dll','System.Core.dll','System.Speech.dll') -TypeDefinition @'
using System;
using System.Collections.Concurrent;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Speech.AudioFormat;
using System.Speech.Recognition;
using System.Threading;

namespace MeetingCopilot {{
  public enum EDataFlow {{ eRender = 0, eCapture = 1, eAll = 2 }}
  public enum ERole {{ eConsole = 0, eMultimedia = 1, eCommunications = 2 }}

  [Flags]
  public enum CLSCTX : uint {{ InprocServer = 0x1, InprocHandler = 0x2, LocalServer = 0x4, All = 0x17 }}

  [StructLayout(LayoutKind.Sequential)]
  public struct WaveFormatEx {{
    public ushort wFormatTag;
    public ushort nChannels;
    public uint nSamplesPerSec;
    public uint nAvgBytesPerSec;
    public ushort nBlockAlign;
    public ushort wBitsPerSample;
    public ushort cbSize;
  }}

  [ComImport]
  [Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")]
  public class MMDeviceEnumeratorComObject {{ }}

  [ComImport]
  [Guid("A95664D2-9614-4F35-A746-DE8DB63617E6")]
  [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  public interface IMMDeviceEnumerator {{
    int EnumAudioEndpoints(EDataFlow dataFlow, uint stateMask, out IntPtr devices);
    int GetDefaultAudioEndpoint(EDataFlow dataFlow, ERole role, out IMMDevice endpoint);
    int GetDevice([MarshalAs(UnmanagedType.LPWStr)] string id, out IMMDevice device);
    int RegisterEndpointNotificationCallback(IntPtr client);
    int UnregisterEndpointNotificationCallback(IntPtr client);
  }}

  [ComImport]
  [Guid("D666063F-1587-4E43-81F1-B948E807363F")]
  [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  public interface IMMDevice {{
    int Activate(ref Guid iid, CLSCTX clsCtx, IntPtr activationParams, [MarshalAs(UnmanagedType.IUnknown)] out object interfacePointer);
    int OpenPropertyStore(int stgmAccess, out IntPtr properties);
    int GetId([MarshalAs(UnmanagedType.LPWStr)] out string id);
    int GetState(out int state);
  }}

  [ComImport]
  [Guid("1CB9AD4C-DBFA-4c32-B178-C2F568A703B2")]
  [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  public interface IAudioClient {{
    int Initialize(int shareMode, int streamFlags, long bufferDuration, long periodicity, IntPtr waveFormat, IntPtr audioSessionGuid);
    int GetBufferSize(out uint bufferSize);
    int GetStreamLatency(out long latency);
    int GetCurrentPadding(out uint padding);
    int IsFormatSupported(int shareMode, IntPtr waveFormat, IntPtr closestMatch);
    int GetMixFormat(out IntPtr waveFormat);
    int GetDevicePeriod(out long defaultPeriod, out long minimumPeriod);
    int Start();
    int Stop();
    int Reset();
    int SetEventHandle(IntPtr eventHandle);
    int GetService(ref Guid iid, [MarshalAs(UnmanagedType.IUnknown)] out object service);
  }}

  [ComImport]
  [Guid("C8ADBD64-E71E-48a0-A4DE-185C395CD317")]
  [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  public interface IAudioCaptureClient {{
    int GetBuffer(out IntPtr data, out uint frames, out uint flags, out ulong devicePosition, out ulong qpcPosition);
    int ReleaseBuffer(uint frames);
    int GetNextPacketSize(out uint frames);
  }}

  public sealed class BlockingAudioStream : Stream {{
    private readonly BlockingCollection<byte[]> queue = new BlockingCollection<byte[]>(64);
    private byte[] current = Array.Empty<byte>();
    private int offset = 0;
    public override bool CanRead {{ get {{ return true; }} }}
    public override bool CanSeek {{ get {{ return false; }} }}
    public override bool CanWrite {{ get {{ return true; }} }}
    public override long Length {{ get {{ throw new NotSupportedException(); }} }}
    public override long Position {{ get {{ throw new NotSupportedException(); }} set {{ throw new NotSupportedException(); }} }}
    public void Complete() {{ queue.CompleteAdding(); }}
    public override void Flush() {{ }}
    public override long Seek(long offset, SeekOrigin origin) {{ throw new NotSupportedException(); }}
    public override void SetLength(long value) {{ throw new NotSupportedException(); }}
    public override void Write(byte[] buffer, int offset, int count) {{
      var copy = new byte[count];
      Buffer.BlockCopy(buffer, offset, copy, 0, count);
      if (!queue.IsAddingCompleted) queue.Add(copy);
    }}
    public override int Read(byte[] buffer, int offset, int count) {{
      var written = 0;
      while (written == 0) {{
        if (this.offset >= current.Length) {{
          try {{
            current = queue.Take();
            this.offset = 0;
          }} catch (InvalidOperationException) {{
            return 0;
          }}
        }}
        var available = Math.Min(count - written, current.Length - this.offset);
        Buffer.BlockCopy(current, this.offset, buffer, offset + written, available);
        this.offset += available;
        written += available;
      }}
      return written;
    }}
  }}

  public sealed class WasapiLoopbackCapture {{
    private const int AUDCLNT_SHAREMODE_SHARED = 0;
    private const int AUDCLNT_STREAMFLAGS_LOOPBACK = 0x00020000;
    private const uint AUDCLNT_BUFFERFLAGS_SILENT = 0x00000002;
    private const ushort WAVE_FORMAT_PCM = 1;
    private const ushort WAVE_FORMAT_IEEE_FLOAT = 3;
    private readonly BlockingAudioStream stream;
    private bool running;

    public WasapiLoopbackCapture(BlockingAudioStream stream) {{
      this.stream = stream;
    }}

    public void Start() {{
      running = true;
      var thread = new Thread(CaptureLoop);
      thread.IsBackground = true;
      thread.Start();
    }}

    private void CaptureLoop() {{
      IAudioClient audioClient = null;
      try {{
        var enumerator = (IMMDeviceEnumerator)new MMDeviceEnumeratorComObject();
        IMMDevice device;
        Check(enumerator.GetDefaultAudioEndpoint(EDataFlow.eRender, ERole.eMultimedia, out device), "GetDefaultAudioEndpoint");
        var iidAudioClient = new Guid("1CB9AD4C-DBFA-4c32-B178-C2F568A703B2");
        object audioClientObject;
        Check(device.Activate(ref iidAudioClient, CLSCTX.All, IntPtr.Zero, out audioClientObject), "IMMDevice.Activate(IAudioClient)");
        audioClient = (IAudioClient)audioClientObject;

        IntPtr mixFormatPtr;
        Check(audioClient.GetMixFormat(out mixFormatPtr), "IAudioClient.GetMixFormat");
        var mixFormat = (WaveFormatEx)Marshal.PtrToStructure(mixFormatPtr, typeof(WaveFormatEx));
        Check(audioClient.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 10000000, 0, mixFormatPtr, IntPtr.Zero), "IAudioClient.Initialize(loopback)");
        var iidCaptureClient = new Guid("C8ADBD64-E71E-48a0-A4DE-185C395CD317");
        object captureClientObject;
        Check(audioClient.GetService(ref iidCaptureClient, out captureClientObject), "IAudioClient.GetService(IAudioCaptureClient)");
        var captureClient = (IAudioCaptureClient)captureClientObject;
        Check(audioClient.Start(), "IAudioClient.Start");
        Console.Error.WriteLine("Windows WASAPI loopback capture started: " + mixFormat.nSamplesPerSec + "Hz/" + mixFormat.nChannels + "ch/" + mixFormat.wBitsPerSample + "bit/tag" + mixFormat.wFormatTag);

        while (running) {{
          uint packetFrames;
          Check(captureClient.GetNextPacketSize(out packetFrames), "IAudioCaptureClient.GetNextPacketSize");
          if (packetFrames == 0) {{
            Thread.Sleep(10);
            continue;
          }}
          while (packetFrames > 0) {{
            IntPtr data;
            uint frames;
            uint flags;
            ulong devicePosition;
            ulong qpcPosition;
            Check(captureClient.GetBuffer(out data, out frames, out flags, out devicePosition, out qpcPosition), "IAudioCaptureClient.GetBuffer");
            var pcm16 = ConvertTo16kMonoPcm(data, frames, flags, mixFormat);
            if (pcm16.Length > 0) stream.Write(pcm16, 0, pcm16.Length);
            Check(captureClient.ReleaseBuffer(frames), "IAudioCaptureClient.ReleaseBuffer");
            Check(captureClient.GetNextPacketSize(out packetFrames), "IAudioCaptureClient.GetNextPacketSize");
          }}
        }}
      }} catch (Exception error) {{
        Console.Error.WriteLine("Windows WASAPI loopback capture failed: " + error);
        stream.Complete();
      }} finally {{
        if (audioClient != null) audioClient.Stop();
      }}
    }}

    private static byte[] ConvertTo16kMonoPcm(IntPtr data, uint frames, uint flags, WaveFormatEx format) {{
      if (frames == 0 || data == IntPtr.Zero) return Array.Empty<byte>();
      var inputRate = (int)format.nSamplesPerSec;
      var channels = Math.Max(1, (int)format.nChannels);
      var outputFrames = Math.Max(1, (int)Math.Round(frames * 16000.0 / inputRate));
      var output = new byte[outputFrames * 2];
      for (int i = 0; i < outputFrames; i++) {{
        var sourceFrame = Math.Min((int)frames - 1, (int)Math.Round(i * inputRate / 16000.0));
        var mono = 0.0;
        if ((flags & AUDCLNT_BUFFERFLAGS_SILENT) != 0) {{
          mono = 0.0;
        }} else {{
          for (int ch = 0; ch < channels; ch++) mono += SampleAt(data, sourceFrame, ch, channels, format);
          mono /= channels;
        }}
        var sample = (short)Math.Max(short.MinValue, Math.Min(short.MaxValue, Math.Round(mono * 32767.0)));
        output[i * 2] = (byte)(sample & 0xff);
        output[i * 2 + 1] = (byte)((sample >> 8) & 0xff);
      }}
      return output;
    }}

    private static double SampleAt(IntPtr data, int frame, int channel, int channels, WaveFormatEx format) {{
      var index = frame * channels + channel;
      if (format.wFormatTag == WAVE_FORMAT_IEEE_FLOAT && format.wBitsPerSample == 32) {{
        return Math.Max(-1.0, Math.Min(1.0, Marshal.PtrToStructure<float>(IntPtr.Add(data, index * 4))));
      }}
      if (format.wFormatTag == WAVE_FORMAT_PCM && format.wBitsPerSample == 16) {{
        return Marshal.PtrToStructure<short>(IntPtr.Add(data, index * 2)) / 32768.0;
      }}
      if (format.wBitsPerSample == 32) {{
        return Math.Max(-1.0, Math.Min(1.0, Marshal.PtrToStructure<float>(IntPtr.Add(data, index * 4))));
      }}
      if (format.wBitsPerSample == 16) {{
        return Marshal.PtrToStructure<short>(IntPtr.Add(data, index * 2)) / 32768.0;
      }}
      throw new NotSupportedException("unsupported Windows mix format: tag=" + format.wFormatTag + " bits=" + format.wBitsPerSample);
    }}

    private static void Check(int hr, string operation) {{
      if (hr < 0) Marshal.ThrowExceptionForHR(hr);
    }}
  }}

  public static class SpeechLoop {{
    public static void Run(string language) {{
      var audio = new BlockingAudioStream();
      var capture = new WasapiLoopbackCapture(audio);
      capture.Start();
      var recognizer = new SpeechRecognitionEngine(CultureInfo.GetCultureInfo(language));
      recognizer.LoadGrammar(new DictationGrammar());
      recognizer.SetInputToAudioStream(audio, new SpeechAudioFormatInfo(16000, AudioBitsPerSample.Sixteen, AudioChannel.Mono));
      var started = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
      while (true) {{
        var result = recognizer.Recognize();
        if (result != null && result.Text.Trim().Length > 0) {{
          var now = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
          var payload = "{{\"kind\":\"transcript\",\"text\":\"" + JsonEscape(result.Text) + "\",\"isFinal\":true,\"confidence\":" + result.Confidence.ToString(CultureInfo.InvariantCulture) + ",\"language\":\"" + JsonEscape(language) + "\",\"source\":\"system\",\"startedAtMs\":" + Math.Max(0, now - started - 3000) + ",\"endedAtMs\":" + Math.Max(0, now - started) + "}}";
          Console.WriteLine(payload);
          Console.Out.Flush();
        }}
      }}
    }}

    private static string JsonEscape(string value) {{
      return value.Replace("\\", "\\\\").Replace("\"", "\\\"").Replace("\r", "\\r").Replace("\n", "\\n");
    }}
  }}
}}
'@

[MeetingCopilot.SpeechLoop]::Run('{language}')
"#
    )
}

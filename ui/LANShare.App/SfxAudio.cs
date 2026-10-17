using NAudio.CoreAudioApi;
using NAudio.Vorbis;
using NAudio.Wave;
using NAudio.Wave.SampleProviders;

namespace LANShare.App;

internal enum SfxFormat { Mp3, Wav, Ogg }

/// Decodifica um efeito sonoro (.mp3, .wav ou .ogg) para PCM float, 48 kHz, estéreo.
/// É o mesmo formato que o Opus usa: na etapa de rede, este PCM é o que vai ser enviado.
internal static class SfxDecoder
{
    public const int SampleRate = 48_000;
    public const int Channels = 2;

    public static readonly TimeSpan MaxDuration = TimeSpan.FromSeconds(5);

    /// Folga: encoders de MP3 costumam acrescentar alguns milissegundos de silêncio.
    private static readonly TimeSpan Tolerance = TimeSpan.FromMilliseconds(150);

    /// Descobre o formato pelos primeiros bytes. Null = não é um formato aceito.
    public static SfxFormat? Detect(ReadOnlySpan<byte> data)
    {
        if (data.Length >= 12
            && data[0] == 'R' && data[1] == 'I' && data[2] == 'F' && data[3] == 'F'
            && data[8] == 'W' && data[9] == 'A' && data[10] == 'V' && data[11] == 'E')
            return SfxFormat.Wav;

        if (data.Length >= 4 && data[0] == 'O' && data[1] == 'g' && data[2] == 'g' && data[3] == 'S')
            return SfxFormat.Ogg;

        // MP3: com etiqueta ID3 no começo ou direto no primeiro quadro (11 bits ligados)
        if (data.Length >= 3 && data[0] == 'I' && data[1] == 'D' && data[2] == '3')
            return SfxFormat.Mp3;
        if (data.Length >= 2 && data[0] == 0xFF && (data[1] & 0xE0) == 0xE0)
            return SfxFormat.Mp3;

        return null;
    }

    public static string Extension(SfxFormat format) => format switch
    {
        SfxFormat.Mp3 => ".mp3",
        SfxFormat.Wav => ".wav",
        _ => ".ogg",
    };

    /// Duração de amostras decodificadas (48 kHz, estéreo, intercaladas).
    public static TimeSpan DurationOf(float[] samples) =>
        TimeSpan.FromSeconds((double)samples.Length / (SampleRate * Channels));

    /// Lê um som da biblioteca e decodifica.
    public static float[] DecodeFile(string path)
    {
        var data = File.ReadAllBytes(path);
        var format = Detect(data) ?? throw new SfxException(Loc.T("Sfx_Unsupported"));
        return Decode(data, format);
    }

    /// Decodifica e confere a duração. Devolve as amostras intercaladas (esquerda, direita...).
    /// Erros viram SfxException com a mensagem no idioma do app.
    public static float[] Decode(byte[] data, SfxFormat format)
    {
        try
        {
            using var reader = Open(new MemoryStream(data, writable: false), format);

            if (reader.TotalTime > MaxDuration + Tolerance) throw TooLong(reader.TotalTime);

            ISampleProvider samples = reader.ToSampleProvider();
            samples = samples.WaveFormat.Channels switch
            {
                1 => new MonoToStereoSampleProvider(samples),
                2 => samples,
                _ => throw new SfxException(Loc.T("Sfx_Unsupported")), // 5.1, 7.1...
            };
            if (samples.WaveFormat.SampleRate != SampleRate)
                samples = new WdlResamplingSampleProvider(samples, SampleRate);

            // Lê tudo, com um teto (a duração informada pelo arquivo pode estar errada)
            var limit = (int)((MaxDuration + Tolerance).TotalSeconds * SampleRate * Channels);
            var output = new List<float>(SampleRate * Channels);
            var buffer = new float[SampleRate * Channels / 10]; // 100 ms
            int read;
            while ((read = samples.Read(buffer, 0, buffer.Length)) > 0)
            {
                output.AddRange(new ArraySegment<float>(buffer, 0, read));
                if (output.Count > limit) throw TooLong(DurationOf(output.ToArray()));
            }

            if (output.Count < Channels) throw new SfxException(Loc.T("Sfx_EmptySound"));
            return output.ToArray();
        }
        catch (SfxException)
        {
            throw;
        }
        catch (Exception ex)
        {
            // Arquivo quebrado, WAV com codec estranho (ADPCM...), etc.
            throw new SfxException(Loc.T("Sfx_OpenError"), ex);
        }
    }

    private static WaveStream Open(Stream stream, SfxFormat format) => format switch
    {
        SfxFormat.Mp3 => new Mp3FileReader(stream),
        SfxFormat.Wav => new WaveFileReader(stream),
        _ => new VorbisWaveReader(stream),
    };

    private static SfxException TooLong(TimeSpan duration) =>
        new(Loc.F("Sfx_TooLong", duration.TotalSeconds));
}

/// Toca os efeitos sonoros na saída de áudio escolhida nas Configurações
/// (a mesma das transmissões; sem escolha ou desconectada = padrão do Windows).
/// Cada som toca na sua própria saída, então vários sons se misturam sozinhos.
internal static class SfxPlayer
{
    /// Mais que isso tocando ao mesmo tempo: o mais antigo para.
    public const int MaxSimultaneous = 4;

    /// Quantos sons decodificados ficam guardados em memória (cada um tem até ~2 MB).
    private const int CacheSize = 12;

    /// Latência do WASAPI, em milissegundos.
    private const int LatencyMs = 60;

    private static readonly object Gate = new();
    private static readonly List<WasapiOut> Playing = new();
    private static readonly Dictionary<string, float[]> Cache = new();
    private static readonly Queue<string> CacheOrder = new();

    /// Ganho já calculado (0.0 a 1.0). Lido pelos sons que estão tocando: mudar o volume vale na hora.
    private static volatile float _gain = 0.49f;

    /// Volume dos efeitos, de 0 a 100. Curva quadrática, como o volume da transmissão.
    public static void SetVolume(int percent)
    {
        var linear = Math.Clamp(percent, 0, 100) / 100f;
        _gain = linear * linear;
    }

    /// Toca um som da biblioteca. Decodifica fora da thread da interface. Devolve a duração do som.
    public static Task<TimeSpan> PlayFileAsync(string path, string? deviceId) =>
        Task.Run(() => Play(Load(path), deviceId));

    /// Decodifica um som da biblioteca (ou pega da memória), fora da thread da interface.
    /// É esse PCM que vai para a sala.
    public static Task<float[]> LoadAsync(string path) => Task.Run(() => Load(path));

    /// Toca amostras já decodificadas, fora da thread da interface (ex.: a prévia ao adicionar um som).
    public static Task<TimeSpan> PlaySamplesAsync(float[] samples, string? deviceId) =>
        Task.Run(() => Play(samples, deviceId));

    /// Toca amostras já decodificadas (48 kHz, estéreo, intercaladas). Devolve a duração do som.
    public static TimeSpan Play(float[] samples, string? deviceId)
    {
        using var enumerator = new MMDeviceEnumerator();
        var device = ResolveDevice(enumerator, deviceId)
                     ?? throw new SfxException(Loc.T("Sfx_NoOutput"));

        var output = new WasapiOut(device, AudioClientShareMode.Shared, true, LatencyMs);
        output.Init(new ClipProvider(samples));

        output.PlaybackStopped += (_, _) =>
        {
            lock (Gate) Playing.Remove(output);
            // Libera fora da thread de reprodução do NAudio (que é quem dispara este evento)
            Task.Run(output.Dispose);
        };

        WasapiOut? oldest = null;
        lock (Gate)
        {
            if (Playing.Count >= MaxSimultaneous)
            {
                oldest = Playing[0];
                Playing.RemoveAt(0);
            }
            Playing.Add(output);
        }

        // Fora do lock: o Stop espera a thread do som, que também usa o lock no PlaybackStopped
        oldest?.Stop();
        output.Play();

        return SfxDecoder.DurationOf(samples);
    }

    /// Para todos os sons (ao fechar o app).
    public static void StopAll()
    {
        List<WasapiOut> all;
        lock (Gate)
        {
            all = Playing.ToList();
            Playing.Clear();
        }

        foreach (var output in all)
        {
            try
            {
                output.Stop();
            }
            catch (Exception)
            {
                // a saída já tinha sumido
            }
        }
    }

    /// Esquece a versão decodificada de um arquivo (ele foi removido).
    public static void Forget(string path)
    {
        lock (Gate) Cache.Remove(path);
    }

    private static float[] Load(string path)
    {
        lock (Gate)
        {
            if (Cache.TryGetValue(path, out var cached)) return cached;
        }

        var samples = SfxDecoder.DecodeFile(path);

        lock (Gate)
        {
            if (!Cache.ContainsKey(path))
            {
                Cache[path] = samples;
                CacheOrder.Enqueue(path);
                while (Cache.Count > CacheSize && CacheOrder.TryDequeue(out var old)) Cache.Remove(old);
            }
        }
        return samples;
    }

    /// A saída escolhida, se estiver conectada; senão a padrão do Windows. Null = o PC não tem saída.
    private static MMDevice? ResolveDevice(MMDeviceEnumerator enumerator, string? deviceId)
    {
        if (!string.IsNullOrEmpty(deviceId))
        {
            try
            {
                var device = enumerator.GetDevice(deviceId);
                if (device.State == DeviceState.Active) return device;
            }
            catch (Exception)
            {
                // o dispositivo salvo não existe mais: cai no padrão
            }
        }

        try
        {
            return enumerator.GetDefaultAudioEndpoint(DataFlow.Render, Role.Console);
        }
        catch (Exception)
        {
            return null;
        }
    }

    /// Entrega as amostras ao WASAPI aplicando o volume atual.
    private sealed class ClipProvider(float[] samples) : ISampleProvider
    {
        private int _position;

        public WaveFormat WaveFormat { get; } =
            WaveFormat.CreateIeeeFloatWaveFormat(SfxDecoder.SampleRate, SfxDecoder.Channels);

        public int Read(float[] buffer, int offset, int count)
        {
            var take = Math.Min(count, samples.Length - _position);
            if (take <= 0) return 0;

            var gain = _gain;
            for (var i = 0; i < take; i++)
                buffer[offset + i] = samples[_position + i] * gain;

            _position += take;
            return take;
        }
    }
}

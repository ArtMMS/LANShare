using Avalonia.Platform;
using NAudio.Wave;

namespace LANShare.App;

internal enum Sound { StreamStarted, UserJoined, UserLeft }

/// Sons do app (assets\sounds, embutidos no exe). Cada som toca na sua própria saída de áudio.
internal static class Sounds
{
    /// O mesmo som não repete antes disso (evita barulho quando várias pessoas entram juntas).
    private static readonly TimeSpan MinRepeat = TimeSpan.FromMilliseconds(300);

    private static readonly Dictionary<Sound, string> Files = new()
    {
        [Sound.StreamStarted] = "start_streaming.mp3",
        [Sound.UserJoined] = "user_join.mp3",
        [Sound.UserLeft] = "user_leave.mp3",
    };

    private static readonly Dictionary<Sound, byte[]?> Cache = new();
    private static readonly Dictionary<Sound, DateTime> LastPlayed = new();

    /// Volume de 0 a 1 (vai para as configurações depois).
    public static float Volume { get; set; } = 0.7f;
    public static bool Enabled { get; set; } = true;

    /// Toca o som sem travar a interface. Arquivo faltando = silêncio.
    public static void Play(Sound sound)
    {
        if (!Enabled) return;

        var now = DateTime.UtcNow;
        if (LastPlayed.TryGetValue(sound, out var last) && now - last < MinRepeat) return;
        LastPlayed[sound] = now;

        var data = Load(sound);
        if (data is null) return;

        var volume = Volume;
        Task.Run(() => PlayBytes(data, volume));
    }

    private static byte[]? Load(Sound sound)
    {
        if (Cache.TryGetValue(sound, out var cached)) return cached;

        byte[]? data = null;
        try
        {
            using var stream = AssetLoader.Open(new Uri($"avares://LANShare/Assets/sounds/{Files[sound]}"));
            using var memory = new MemoryStream();
            stream.CopyTo(memory);
            data = memory.ToArray();
        }
        catch (Exception)
        {
            // arquivo não existe: fica sem som
        }

        Cache[sound] = data;
        return data;
    }

    private static void PlayBytes(byte[] data, float volume)
    {
        try
        {
            var reader = new Mp3FileReader(new MemoryStream(data));
            var output = new WaveOutEvent();
            output.Init(reader);
            output.Volume = Math.Clamp(volume, 0f, 1f);

            // Libera tudo quando o som termina
            output.PlaybackStopped += (_, _) =>
            {
                output.Dispose();
                reader.Dispose();
            };
            output.Play();
        }
        catch (Exception)
        {
            // sem dispositivo de áudio ou MP3 inválido: ignora
        }
    }
}
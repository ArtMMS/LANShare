using System.Text.Json;
using System.Text.Json.Serialization;
using Avalonia.Input;

namespace LANShare.App;

/// Preferências da interface, em %APPDATA%\LANShare\preferences.json.
/// (O settings.json continua sendo do motor Rust, só com o nome de usuário.)
internal sealed class AppPreferences
{
    /// Idioma do app: "pt", "en" ou "es". Null = idioma do Windows (inglês se não for pt/es).
    /// Só vale depois de reiniciar o app.
    public string? Language { get; set; }

    /// Tecla da tela cheia (nome da tecla no Avalonia). Null = sem atalho.
    public string? FullscreenKey { get; set; } = nameof(Key.F11);

    /// Resolução e FPS que já vêm selecionados ao abrir o app.
    public int StreamHeight { get; set; } = 1080;
    public int StreamFps { get; set; } = 30;

    /// Botão 🔊 do seletor de monitor/janela: a transmissão leva som? Lembra a última escolha.
    /// Monitor inteiro = som do PC; janela = só o som do app dela.
    public bool StreamAudio { get; set; } = true;

    /// Apps que não transmitem som ao transmitir um monitor inteiro (Discord, Teams...).
    /// Os da lista padrão vêm ligados; os adicionados pelo usuário podem ser removidos.
    public List<ExcludedAudioApp> ExcludedAudioApps { get; set; } = ExcludedAudioApp.Defaults();

    /// Os .exe que o motor deve deixar de fora (só os ligados), em minúsculas.
    [JsonIgnore]
    public List<string> ExcludedExes =>
        ExcludedAudioApps.Where(a => a.Enabled)
            .SelectMany(a => a.Exes)
            .Select(e => e.Trim().ToLowerInvariant())
            .Where(e => e.Length > 0)
            .Distinct()
            .ToList();

    /// Saída do som das transmissões que o Client assiste (e dos efeitos sonoros): o id do
    /// dispositivo no Windows. Null = padrão do Windows (acompanha se o padrão mudar). Se o
    /// dispositivo salvo não estiver conectado, o som sai pelo padrão, mas a escolha continua salva.
    public string? AudioOutputDevice { get; set; }

    /// Volume do som das transmissões no Client, de 0 a 100. Começa na metade.
    public int StreamVolume { get; set; } = 50;

    /// Som das transmissões no mudo (o volume continua guardado para quando voltar).
    public bool StreamMuted { get; set; }

    /// Volume dos efeitos sonoros (soundboard) que chegam para este usuário, de 0 a 100.
    public int SfxVolume { get; set; } = 70;

    public bool SoundOnUserJoinLeave { get; set; } = true;
    public bool SoundOnStreamStart { get; set; } = true;

    /// Procurar versão nova no GitHub ao abrir o app.
    public bool AutoCheckUpdates { get; set; } = true;

    private static readonly JsonSerializerOptions JsonOptions = new() { WriteIndented = true };

    private static string Folder =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "LANShare");

    private static string FilePath => Path.Combine(Folder, "preferences.json");

    /// A tecla da tela cheia já convertida (null = sem atalho).
    [JsonIgnore]
    public Key? FullscreenKeyValue
    {
        get => Enum.TryParse<Key>(FullscreenKey, out var key) && key != Key.None ? key : null;
        set => FullscreenKey = value?.ToString();
    }

    /// Lê o arquivo; ausente ou quebrado = valores padrão.
    public static AppPreferences Load()
    {
        try
        {
            if (File.Exists(FilePath))
            {
                var prefs = JsonSerializer.Deserialize<AppPreferences>(File.ReadAllText(FilePath), JsonOptions);
                if (prefs is not null)
                {
                    prefs.Normalize();
                    return prefs;
                }
            }
        }
        catch (Exception)
        {
            // arquivo corrompido: usa os padrões
        }

        return new AppPreferences();
    }

    /// Salva (num temporário e troca, para não corromper se falhar no meio).
    public void Save()
    {
        try
        {
            Directory.CreateDirectory(Folder);
            var temp = FilePath + ".tmp";
            File.WriteAllText(temp, JsonSerializer.Serialize(this, JsonOptions));
            File.Move(temp, FilePath, overwrite: true);
        }
        catch (Exception)
        {
            // sem permissão de escrita: a preferência vale só até fechar o app
        }
    }

    /// Valores fora do permitido voltam ao padrão.
    private void Normalize()
    {
        if (StreamHeight is not (720 or 1080)) StreamHeight = 1080;
        if (StreamFps is not (15 or 30)) StreamFps = 30;
        if (Loc.FromCode(Language) is null) Language = null;
        if (string.IsNullOrWhiteSpace(AudioOutputDevice)) AudioOutputDevice = null;
        StreamVolume = Math.Clamp(StreamVolume, 0, 100);
        SfxVolume = Math.Clamp(SfxVolume, 0, 100);
        ExcludedAudioApps = ExcludedAudioApp.Normalize(ExcludedAudioApps);
    }
}

/// Um app que não transmite som (lista das Configurações).
internal sealed class ExcludedAudioApp
{
    /// Identificador: o da lista padrão (ex.: "discord") ou, para apps adicionados, o .exe.
    public string Id { get; set; } = "";

    /// Nome mostrado nas Configurações.
    public string Name { get; set; } = "";

    /// Os .exe do app (um app pode ter mais de um: Discord, Discord PTB, Discord Canary...).
    public List<string> Exes { get; set; } = new();

    /// true = o som desse app NÃO vai na transmissão.
    public bool Enabled { get; set; } = true;

    /// true = veio na lista padrão (dá para desligar, mas não remover).
    public bool BuiltIn { get; set; }

    /// Apps de chamada mais usados, já bloqueados.
    public static List<ExcludedAudioApp> Defaults() => new()
    {
        BuiltInApp("discord", "Discord", "discord.exe", "discordptb.exe", "discordcanary.exe"),
        BuiltInApp("teams", "Microsoft Teams", "ms-teams.exe", "teams.exe", "msteams.exe"),
        BuiltInApp("teamspeak", "TeamSpeak", "teamspeak.exe", "ts3client_win64.exe", "ts3client_win32.exe"),
        BuiltInApp("zoom", "Zoom", "zoom.exe"),
        BuiltInApp("skype", "Skype", "skype.exe", "skypeapp.exe"),
    };

    /// Um app adicionado pelo usuário (pela lista de apps com som ou escolhendo o .exe).
    public static ExcludedAudioApp Custom(string name, string exe)
    {
        var cleanExe = exe.Trim().ToLowerInvariant();
        return new ExcludedAudioApp
        {
            Id = cleanExe,
            Name = string.IsNullOrWhiteSpace(name) ? cleanExe : name.Trim(),
            Exes = new List<string> { cleanExe },
            Enabled = true,
            BuiltIn = false,
        };
    }

    /// Esse .exe já está na lista (em qualquer app)?
    public static bool Contains(IEnumerable<ExcludedAudioApp> list, string exe) =>
        list.Any(a => a.Exes.Any(e => string.Equals(e, exe, StringComparison.OrdinalIgnoreCase)));

    /// Arruma a lista lida do arquivo: os da lista padrão sempre existem (com os .exe atualizados,
    /// guardando só se estão ligados) e os adicionados sem .exe ou repetidos saem.
    public static List<ExcludedAudioApp> Normalize(List<ExcludedAudioApp>? saved)
    {
        saved ??= new List<ExcludedAudioApp>();
        var result = new List<ExcludedAudioApp>();

        foreach (var builtIn in Defaults())
        {
            var old = saved.FirstOrDefault(a => a.BuiltIn && a.Id == builtIn.Id);
            if (old is not null) builtIn.Enabled = old.Enabled;
            result.Add(builtIn);
        }

        foreach (var app in saved.Where(a => !a.BuiltIn))
        {
            var exe = app.Exes?.FirstOrDefault(e => !string.IsNullOrWhiteSpace(e));
            if (exe is null || Contains(result, exe)) continue;

            var custom = Custom(app.Name, exe);
            custom.Enabled = app.Enabled;
            result.Add(custom);
        }

        return result;
    }

    private static ExcludedAudioApp BuiltInApp(string id, string name, params string[] exes) => new()
    {
        Id = id,
        Name = name,
        Exes = exes.ToList(),
        Enabled = true,
        BuiltIn = true,
    };
}
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

    /// Saída do som das transmissões que o Client assiste: o id do dispositivo no Windows.
    /// Null = padrão do Windows (acompanha se o padrão mudar). Se o dispositivo salvo
    /// não estiver conectado, o som sai pelo padrão, mas a escolha continua salva.
    public string? AudioOutputDevice { get; set; }

    /// Volume do som das transmissões no Client, de 0 a 100. Começa na metade.
    public int StreamVolume { get; set; } = 50;

    /// Som das transmissões no mudo (o volume continua guardado para quando voltar).
    public bool StreamMuted { get; set; }

    public bool SoundOnUserJoinLeave { get; set; } = true;
    public bool SoundOnStreamStart { get; set; } = true;

    /// Ainda não usado: vale quando as atualizações automáticas chegarem.
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
    }
}

using System.Text.Json;
using System.Text.Json.Serialization;
using Avalonia.Input;

namespace LANShare.App;

/// Preferências da interface, em %APPDATA%\LANShare\preferences.json.
/// (O settings.json continua sendo do motor Rust, só com o nome de usuário.)
internal sealed class AppPreferences
{
    /// Tecla da tela cheia (nome da tecla no Avalonia). Null = sem atalho.
    public string? FullscreenKey { get; set; } = nameof(Key.F11);

    /// Resolução e FPS que já vêm selecionados ao abrir o app.
    public int StreamHeight { get; set; } = 1080;
    public int StreamFps { get; set; } = 30;

    /// Dispositivo de saída do áudio das transmissões (null = padrão do Windows). Ainda não usado.
    public string? AudioOutputDevice { get; set; }

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
    }
}
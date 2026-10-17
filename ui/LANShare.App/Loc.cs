using System.Globalization;
using Avalonia.Markup.Xaml;

namespace LANShare.App;

internal enum AppLanguage { Portuguese, English, Spanish }

/// Textos da interface em português, inglês e espanhol.
/// O idioma é escolhido uma vez, ao abrir o app (trocar nas Configurações pede reinício).
internal static class Loc
{
    public static AppLanguage Current { get; private set; } = AppLanguage.Portuguese;

    /// "pt", "en" ou "es" (também é o que vai para o motor Rust e para o preferences.json).
    public static string Code => CodeOf(Current);

    public static readonly AppLanguage[] All = { AppLanguage.Portuguese, AppLanguage.English, AppLanguage.Spanish };

    public static string CodeOf(AppLanguage language) => language switch
    {
        AppLanguage.English => "en",
        AppLanguage.Spanish => "es",
        _ => "pt",
    };

    /// Nome do idioma escrito nele mesmo (aparece igual em qualquer idioma do app).
    public static string NativeName(AppLanguage language) => language switch
    {
        AppLanguage.English => "English",
        AppLanguage.Spanish => "Español",
        _ => "Português",
    };

    public static AppLanguage? FromCode(string? code) => code?.Trim().ToLowerInvariant() switch
    {
        "pt" => AppLanguage.Portuguese,
        "en" => AppLanguage.English,
        "es" => AppLanguage.Spanish,
        _ => null,
    };

    /// Idioma do Windows: português e espanhol têm versão própria; o resto usa inglês.
    public static AppLanguage FromSystem() => CultureInfo.CurrentUICulture.TwoLetterISOLanguageName switch
    {
        "pt" => AppLanguage.Portuguese,
        "es" => AppLanguage.Spanish,
        _ => AppLanguage.English,
    };

    /// Chamado no Program.Main, antes de qualquer janela. Sem idioma salvo = idioma do Windows.
    public static void Init(string? savedCode) => Current = FromCode(savedCode) ?? FromSystem();

    /// Texto no idioma atual. Chave desconhecida = a própria chave (fácil de achar o erro).
    public static string T(string key) =>
        Table.TryGetValue(key, out var texts) ? texts[(int)Current] : key;

    /// Texto com valores, no formato do string.Format ({0}, {1:F1}...).
    public static string F(string key, params object?[] args) =>
        string.Format(CultureInfo.CurrentCulture, T(key), args);

    // Ordem de cada linha: português, inglês, espanhol.
    // Termos técnicos (FPS, Ping, Mbps, Host, IP...) ficam iguais nos três.
    private static readonly Dictionary<string, string[]> Table = new()
    {
        // ── Geral ──
        ["Common_Cancel"] = new[] { "Cancelar", "Cancel", "Cancelar" },
        ["Common_Confirm"] = new[] { "Confirmar", "Confirm", "Confirmar" },
        ["Common_Save"] = new[] { "Salvar", "Save", "Guardar" },
        ["Common_BackHome"] = new[] { "Voltar ao início", "Back to start", "Volver al inicio" },
        ["Common_Error"] = new[] { "Erro: {0}", "Error: {0}", "Error: {0}" },
        ["Common_Someone"] = new[] { "Alguém", "Someone", "Alguien" },
        ["Link_OpenError"] = new[] { "Não foi possível abrir o link: {0}", "Could not open the link: {0}", "No se pudo abrir el enlace: {0}" },

        // ── Motor Rust ──
        ["Core_Unavailable"] = new[] { "{0} — motor Rust indisponível", "{0} — Rust engine unavailable", "{0} — motor Rust no disponible" },
        ["Core_DllProblem"] = new[]
        {
            "Problema com a lanshare_ffi.dll. Rode: cargo build --release -p lanshare-ffi",
            "Problem with lanshare_ffi.dll. Run: cargo build --release -p lanshare-ffi",
            "Problema con lanshare_ffi.dll. Ejecuta: cargo build --release -p lanshare-ffi",
        },

        // ── Título e apoio ──
        ["Title_Tooltip"] = new[] { "Abrir a página do LANShare no GitHub", "Open the LANShare page on GitHub", "Abrir la página de LANShare en GitHub" },
        ["Support_Button"] = new[] { "Apoie o LANShare", "Support LANShare", "Apoya a LANShare" },

        // ── Nome de usuário ──
        ["Name_Welcome"] = new[] { "Seja bem-vindo ao LANShare", "Welcome to LANShare", "Bienvenido a LANShare" },
        ["Name_Prompt"] = new[] { "Para começar, insira seu nome de usuário", "To get started, enter your username", "Para empezar, introduce tu nombre de usuario" },
        ["Name_Placeholder"] = new[] { "Digite seu nome de usuário...", "Type your username...", "Escribe tu nombre de usuario..." },
        ["Name_Invalid"] = new[] { "Nome inválido.", "Invalid name.", "Nombre no válido." },
        ["Profile_EditNameTooltip"] = new[] { "Clique para editar o nome", "Click to edit your name", "Haz clic para editar el nombre" },

        // ── Início ──
        ["Home_Start"] = new[] { "COMEÇAR", "GET STARTED", "EMPEZAR" },
        ["Home_CreateRoom"] = new[] { "Criar sala", "Create room", "Crear sala" },
        ["Home_JoinRoom"] = new[] { "Entrar em sala", "Join room", "Unirse a una sala" },
        ["Home_ChooseOption"] = new[] { "Escolha uma opção ao lado para começar.", "Choose an option on the side to get started.", "Elige una opción del lateral para empezar." },

        // ── Host ──
        ["Host_StatusHeader"] = new[] { "STATUS", "STATUS", "ESTADO" },
        ["Host_ConnectionHeader"] = new[] { "CONEXÃO", "CONNECTION", "CONEXIÓN" },
        ["Host_RoomNamePlaceholder"] = new[] { "Nome da sala (opcional)", "Room name (optional)", "Nombre de la sala (opcional)" },
        ["Host_PasswordPlaceholder"] = new[] { "Senha (opcional)", "Password (optional)", "Contraseña (opcional)" },
        ["Host_OpenRoom"] = new[] { "Abrir sala", "Open room", "Abrir sala" },
        ["Host_CloseRoom"] = new[] { "Fechar sala", "Close room", "Cerrar sala" },
        ["Host_RoomOpen"] = new[] { "Sala aberta", "Room open", "Sala abierta" },
        ["Host_RoomClosed"] = new[] { "Sala fechada", "Room closed", "Sala cerrada" },
        ["Host_Streaming"] = new[] { "Transmitindo", "Streaming", "Transmitiendo" },
        ["Host_OpenToStart"] = new[] { "Abra a sala para começar.", "Open the room to get started.", "Abre la sala para empezar." },
        ["Host_OpenError"] = new[] { "Erro ao abrir a sala.", "Error opening the room.", "Error al abrir la sala." },
        ["Host_RoomOpenHint"] = new[] { "Sala aberta. Clique em Iniciar transmissão.", "Room open. Click Start stream.", "Sala abierta. Haz clic en Iniciar transmisión." },

        // ── Lista de usuários ──
        ["Room_UsersHeader"] = new[] { "USUÁRIOS NA SALA", "USERS IN THE ROOM", "USUARIOS EN LA SALA" },
        ["Users_YouHost"] = new[] { "{0} (você, host)", "{0} (you, host)", "{0} (tú, host)" },
        ["Users_You"] = new[] { "{0} (você)", "{0} (you)", "{0} (tú)" },
        ["Users_Host"] = new[] { "{0} (host)", "{0} (host)", "{0} (host)" },
        ["Users_Kick"] = new[] { "Expulsar", "Kick", "Expulsar" },

        // ── Transmissão ──
        ["Stream_Start"] = new[] { "Iniciar transmissão", "Start stream", "Iniciar transmisión" },
        ["Stream_Change"] = new[] { "Alterar transmissão", "Change stream", "Cambiar transmisión" },
        ["Stream_Stop"] = new[] { "Parar transmissão", "Stop stream", "Detener transmisión" },
        ["Stream_StartError"] = new[] { "Erro ao iniciar a transmissão.", "Error starting the stream.", "Error al iniciar la transmisión." },
        ["Stream_ChangeError"] = new[] { "Não foi possível alterar a transmissão.", "Could not change the stream.", "No se pudo cambiar la transmisión." },
        ["Stream_ChangedWindow"] = new[] { "Transmissão alterada para a janela.", "Stream switched to the window.", "Transmisión cambiada a la ventana." },
        ["Stream_ChangedMonitor"] = new[] { "Transmissão alterada para o monitor.", "Stream switched to the monitor.", "Transmisión cambiada al monitor." },
        ["Stream_CapturingScreen"] = new[]
        {
            "Capturando a tela. A imagem é enviada quando um usuário entrar.",
            "Capturing the screen. The image is sent when a user joins.",
            "Capturando la pantalla. La imagen se envía cuando entre un usuario.",
        },
        ["Stream_CapturingWindow"] = new[]
        {
            "Capturando a janela. A imagem é enviada quando um usuário entrar.",
            "Capturing the window. The image is sent when a user joins.",
            "Capturando la ventana. La imagen se envía cuando entre un usuario.",
        },
        ["Stream_Started"] = new[] { "Transmitindo {0}x{1} @ {2} FPS", "Streaming {0}x{1} @ {2} FPS", "Transmitiendo {0}x{1} @ {2} FPS" },
        ["Stream_Stats"] = new[] { "Transmitindo — {0:F1} Mbps, {1} usuário(s)", "Streaming — {0:F1} Mbps, {1} user(s)", "Transmitiendo — {0:F1} Mbps, {1} usuario(s)" },
        ["Stream_Interrupted"] = new[] { "Transmissão interrompida: {0}", "Stream interrupted: {0}", "Transmisión interrumpida: {0}" },
        ["Stream_Stopped"] = new[] { "Transmissão parada.", "Stream stopped.", "Transmisión detenida." },

        // ── Avisos (canto da janela) ──
        ["Notify_Joined"] = new[] { "{0} entrou na sala.", "{0} joined the room.", "{0} entró en la sala." },
        ["Notify_Left"] = new[] { "{0} saiu da sala.", "{0} left the room.", "{0} salió de la sala." },
        ["Notify_Kicked"] = new[] { "{0} foi removido da sala.", "{0} was removed from the room.", "{0} fue expulsado de la sala." },
        ["Notify_YouStarted"] = new[] { "Você iniciou a transmissão.", "You started the stream.", "Iniciaste la transmisión." },
        ["Notify_HostStarted"] = new[] { "{0} iniciou a transmissão.", "{0} started the stream.", "{0} inició la transmisión." },

        // ── Client ──
        ["Client_Disconnect"] = new[] { "Desconectar", "Disconnect", "Desconectar" },
        ["Client_Disconnected"] = new[] { "Desconectado", "Disconnected", "Desconectado" },
        ["Client_Connecting"] = new[] { "Conectando...", "Connecting...", "Conectando..." },
        ["Client_Connected"] = new[] { "Conectado", "Connected", "Conectado" },
        ["Client_LeaveAndBack"] = new[] { "Sair da sala e voltar", "Leave room and go back", "Salir de la sala y volver" },
        ["Client_RoomsSection"] = new[] { "SALAS NA REDE", "ROOMS ON THE NETWORK", "SALAS EN LA RED" },
        ["Client_RoomsTitle"] = new[] { "Salas na rede atual", "Rooms on this network", "Salas en la red actual" },
        ["Client_Refresh"] = new[] { "Atualizar", "Refresh", "Actualizar" },
        ["Client_IpSection"] = new[] { "CONEXÃO POR IP", "CONNECT BY IP", "CONEXIÓN POR IP" },
        ["Client_IpTitle"] = new[] { "Ou entre por IP", "Or join by IP", "O entra por IP" },
        ["Client_IpPlaceholder"] = new[] { "IP da sala (ex.: 192.168.0.10)", "Room IP (e.g. 192.168.0.10)", "IP de la sala (ej.: 192.168.0.10)" },
        ["Client_Connect"] = new[] { "Conectar", "Connect", "Conectar" },
        ["Client_RoomHasPassword"] = new[] { "Esta sala tem senha.", "This room has a password.", "Esta sala tiene contraseña." },
        ["Client_PasswordPlaceholder"] = new[] { "Senha da sala", "Room password", "Contraseña de la sala" },
        ["Client_Enter"] = new[] { "Entrar", "Join", "Entrar" },
        ["Client_ChooseRoom"] = new[] { "Escolha uma sala para entrar.", "Choose a room to join.", "Elige una sala para entrar." },
        ["Client_StartError"] = new[] { "Erro ao iniciar o modo cliente.", "Error starting client mode.", "Error al iniciar el modo cliente." },
        ["Client_Searching"] = new[] { "Procurando salas...", "Searching for rooms...", "Buscando salas..." },
        ["Client_ScanError"] = new[] { "Erro ao procurar salas.", "Error searching for rooms.", "Error al buscar salas." },
        ["Client_NoRooms"] = new[] { "Nenhuma sala encontrada na rede.", "No rooms found on the network.", "No se encontraron salas en la red." },
        ["Client_TypeIp"] = new[]
        {
            "Digite o IP da sala ou dê um duplo clique em uma sala da lista.",
            "Type the room IP or double-click a room in the list.",
            "Escribe la IP de la sala o haz doble clic en una sala de la lista.",
        },
        ["Client_IncompatibleRoom"] = new[]
        {
            "Esta sala usa uma versão incompatível do LANShare.",
            "This room uses an incompatible version of LANShare.",
            "Esta sala usa una versión incompatible de LANShare.",
        },
        ["Client_ConnectError"] = new[] { "Erro ao conectar.", "Error connecting.", "Error al conectar." },
        ["Client_ConnectingTo"] = new[] { "Conectando a {0}...", "Connecting to {0}...", "Conectando a {0}..." },
        ["Client_PasswordSendError"] = new[] { "Erro ao enviar a senha.", "Error sending the password.", "Error al enviar la contraseña." },
        ["Client_CheckingPassword"] = new[] { "Verificando a senha...", "Checking the password...", "Verificando la contraseña..." },
        ["Client_ProtectedRoom"] = new[] { "Sala protegida", "Protected room", "Sala protegida" },
        ["Client_WrongPassword"] = new[] { "Senha incorreta. Tente novamente.", "Wrong password. Try again.", "Contraseña incorrecta. Inténtalo de nuevo." },
        ["Client_WaitingStream"] = new[] { "Conectado. Aguardando a transmissão do Host.", "Connected. Waiting for the Host's stream.", "Conectado. Esperando la transmisión del Host." },
        ["Client_KickedDefault"] = new[] { "Você foi removido pelo Host.", "You were removed by the Host.", "El Host te ha expulsado de la sala." },
        ["Client_VideoError"] = new[] { "Erro no vídeo.", "Video error.", "Error de vídeo." },
        ["Client_Rejected"] = new[] { "Entrada recusada: {0}", "Join refused: {0}", "Entrada rechazada: {0}" },
        ["Client_ConnectFailed"] = new[] { "Não foi possível conectar.", "Could not connect.", "No se pudo conectar." },
        ["Client_DisconnectedReason"] = new[] { "Desconectado: {0}", "Disconnected: {0}", "Desconectado: {0}" },

        // ── Cartões de sala ──
        ["Room_DefaultName"] = new[] { "Sala", "Room", "Sala" },
        ["Room_Users"] = new[] { "{0} na sala", "{0} in the room", "{0} en la sala" },
        ["Room_UsersMax"] = new[] { "{0}/{1} na sala", "{0}/{1} in the room", "{0}/{1} en la sala" },
        ["Room_WithPassword"] = new[] { "com senha", "password protected", "con contraseña" },
        ["Room_Incompatible"] = new[] { "versão incompatível", "incompatible version", "versión incompatible" },

        // ── Escolha do que transmitir ──
        ["Picker_MonitorsTab"] = new[] { "Monitores inteiros", "Entire monitors", "Monitores completos" },
        ["Picker_WindowsTab"] = new[] { "Janela específica", "Specific window", "Ventana específica" },
        ["Picker_NoMonitors"] = new[] { "Nenhum monitor encontrado.", "No monitors found.", "No se encontró ningún monitor." },
        ["Picker_NoWindows"] = new[] { "Nenhuma janela aberta para transmitir.", "No open windows to stream.", "No hay ventanas abiertas para transmitir." },
        ["Picker_MonitorN"] = new[] { "Monitor {0}", "Monitor {0}", "Monitor {0}" },
        ["Picker_WindowDefault"] = new[] { "Janela", "Window", "Ventana" },
        ["Picker_Primary"] = new[] { "Principal", "Primary", "Principal" },
        ["Picker_Minimized"] = new[] { "Minimizada", "Minimized", "Minimizada" },

        // ── Avatar ──
        ["Avatar_ChangeTooltip"] = new[] { "Alterar avatar", "Change avatar", "Cambiar avatar" },
        ["Avatar_Title"] = new[] { "Alterar avatar", "Change avatar", "Cambiar avatar" },
        ["Avatar_ClickToChoose"] = new[] { "Clique para escolher uma imagem", "Click to choose an image", "Haz clic para elegir una imagen" },
        ["Avatar_OrDrag"] = new[] { "ou arraste ela para cá", "or drag it here", "o arrástrala aquí" },
        ["Avatar_ChooseAnother"] = new[] { "Escolher outra imagem", "Choose another image", "Elegir otra imagen" },
        ["Avatar_Remove"] = new[] { "Remover avatar", "Remove avatar", "Quitar avatar" },
        ["Avatar_FileType"] = new[] { "Imagens", "Images", "Imágenes" },
        ["Avatar_PickerTitle"] = new[] { "Escolha uma imagem para o avatar", "Choose an image for your avatar", "Elige una imagen para el avatar" },
        ["Avatar_NoGif"] = new[]
        {
            "GIF não é aceito como avatar. Use PNG, JPG, BMP ou WEBP.",
            "GIF is not accepted as an avatar. Use PNG, JPG, BMP or WEBP.",
            "GIF no se acepta como avatar. Usa PNG, JPG, BMP o WEBP.",
        },
        ["Avatar_TooSmall"] = new[] { "Essa imagem é pequena demais.", "This image is too small.", "Esta imagen es demasiado pequeña." },
        ["Avatar_OpenError"] = new[]
        {
            "Não foi possível abrir essa imagem. Use PNG, JPG, BMP ou WEBP.",
            "Could not open this image. Use PNG, JPG, BMP or WEBP.",
            "No se pudo abrir esta imagen. Usa PNG, JPG, BMP o WEBP.",
        },
        ["Avatar_SaveError"] = new[] { "Não foi possível salvar o avatar: {0}", "Could not save the avatar: {0}", "No se pudo guardar el avatar: {0}" },
        ["Avatar_RemoveError"] = new[] { "Não foi possível remover o avatar: {0}", "Could not remove the avatar: {0}", "No se pudo quitar el avatar: {0}" },

        // ── Configurações ──
        ["Settings_Title"] = new[] { "Configurações", "Settings", "Configuración" },
        ["Settings_CloseTooltip"] = new[] { "Fechar (Esc)", "Close (Esc)", "Cerrar (Esc)" },
        ["Settings_Shortcuts"] = new[] { "Atalhos", "Shortcuts", "Atajos" },
        ["Settings_Fullscreen"] = new[] { "Tela cheia", "Fullscreen", "Pantalla completa" },
        ["Settings_ShortcutHint"] = new[]
        {
            "Clique e pressione uma tecla. Esc deixa sem atalho.",
            "Click and press a key. Esc leaves it without a shortcut.",
            "Haz clic y pulsa una tecla. Esc lo deja sin atajo.",
        },
        ["Settings_PressKey"] = new[] { "Pressione uma tecla...", "Press a key...", "Pulsa una tecla..." },
        ["Settings_StreamPrefs"] = new[] { "Preferências de transmissão", "Stream preferences", "Preferencias de transmisión" },
        ["Settings_DefaultResolution"] = new[] { "Resolução padrão", "Default resolution", "Resolución predeterminada" },
        ["Settings_DefaultFps"] = new[] { "FPS padrão", "Default FPS", "FPS predeterminados" },
        ["Settings_StreamHint"] = new[]
        {
            "Já vem selecionado ao abrir o app. Dá para mudar na lateral antes de cada transmissão.",
            "Selected when the app opens. You can change it on the side before each stream.",
            "Viene seleccionado al abrir la app. Puedes cambiarlo en el lateral antes de cada transmisión.",
        },
        ["Settings_AudioNotifications"] = new[] { "Áudio & Notificações", "Audio & Notifications", "Audio y notificaciones" },
        ["Settings_AudioOutput"] = new[] { "Saída do áudio das transmissões", "Stream audio output", "Salida de audio de las transmisiones" },
        ["Settings_WindowsDefault"] = new[] { "Padrão do Windows", "Windows default", "Predeterminado de Windows" },
        ["Settings_WindowsDefaultNamed"] = new[] { "Padrão do Windows ({0})", "Windows default ({0})", "Predeterminado de Windows ({0})" },
        ["Settings_AudioOutputHint"] = new[]
        {
            "Por onde sai o som das transmissões que você assiste. O padrão do Windows acompanha o sistema se ele mudar.",
            "Where the sound of the streams you watch comes out. Windows default follows the system if it changes.",
            "Por dónde sale el sonido de las transmisiones que ves. El predeterminado de Windows sigue al sistema si cambia.",
        },
        ["Settings_AudioDeviceMissing"] = new[]
        {
            "O dispositivo escolhido não está conectado. Enquanto isso, o som sai pelo padrão do Windows.",
            "The chosen device is not connected. Meanwhile, the sound comes out of the Windows default.",
            "El dispositivo elegido no está conectado. Mientras tanto, el sonido sale por el predeterminado de Windows.",
        },
        ["Settings_AudioDevicesError"] = new[]
        {
            "Não foi possível listar os dispositivos de áudio. O som sai pelo padrão do Windows.",
            "Could not list the audio devices. The sound comes out of the Windows default.",
            "No se pudieron listar los dispositivos de audio. El sonido sale por el predeterminado de Windows.",
        },
        ["Settings_NotificationSounds"] = new[] { "Sons de notificação", "Notification sounds", "Sonidos de notificación" },
        ["Settings_SoundJoinLeave"] = new[] { "Quando um usuário entra ou sai", "When a user joins or leaves", "Cuando un usuario entra o sale" },
        ["Settings_SoundStream"] = new[] { "Quando a transmissão começa", "When the stream starts", "Cuando empieza la transmisión" },
        ["Settings_Updates"] = new[] { "Atualizações", "Updates", "Actualizaciones" },
        ["Settings_AutoUpdate"] = new[] { "Verificar atualizações automaticamente", "Check for updates automatically", "Buscar actualizaciones automáticamente" },
        ["Settings_AutoUpdateHint"] = new[]
        {
            "Procura ao abrir o app. Desligado, só procura quando você clicar em Verificar agora.",
            "Checks when the app opens. When off, it only checks when you click Check now.",
            "Busca al abrir la app. Desactivado, solo busca cuando hagas clic en Buscar ahora.",
        },
        ["Settings_CheckNow"] = new[] { "Verificar agora", "Check now", "Buscar ahora" },
        ["Settings_Language"] = new[] { "Idioma", "Language", "Idioma" },
        ["Settings_LanguageHint"] = new[]
        {
            "O novo idioma é aplicado depois de reiniciar o LANShare.",
            "The new language is applied after restarting LANShare.",
            "El nuevo idioma se aplica después de reiniciar LANShare.",
        },
        ["Settings_RestartPending"] = new[]
        {
            "Reinicie o LANShare para aplicar o novo idioma.",
            "Restart LANShare to apply the new language.",
            "Reinicia LANShare para aplicar el nuevo idioma.",
        },
        ["Settings_RestartNow"] = new[] { "Reiniciar agora", "Restart now", "Reiniciar ahora" },
        ["Settings_RestartBlocked"] = new[]
        {
            "Feche a sala ou desconecte antes de reiniciar.",
            "Close the room or disconnect before restarting.",
            "Cierra la sala o desconéctate antes de reiniciar.",
        },
        ["Settings_RestartFailed"] = new[] { "Não foi possível reiniciar: {0}", "Could not restart: {0}", "No se pudo reiniciar: {0}" },
        ["Key_None"] = new[] { "Nenhuma", "None", "Ninguna" },
        ["Key_Space"] = new[] { "Espaço", "Space", "Espacio" },

        // ── Atualizações ──
        ["Update_AvailableTitle"] = new[] { "Nova versão disponível", "New version available", "Nueva versión disponible" },
        ["Update_Later"] = new[] { "Depois", "Later", "Después" },
        ["Update_Now"] = new[] { "Atualizar", "Update", "Actualizar" },
        ["Update_Install"] = new[] { "Instalar", "Install", "Instalar" },
        ["Update_Notes"] = new[] { "Ver novidades", "See what's new", "Ver novedades" },
        ["Update_Updated"] = new[] { "LANShare atualizado para {0}.", "LANShare updated to {0}.", "LANShare actualizado a {0}." },
        ["Update_NewVersionNotify"] = new[] { "Nova versão disponível: {0}.", "New version available: {0}.", "Nueva versión disponible: {0}." },
        ["Update_Checking"] = new[] { "Verificando...", "Checking...", "Buscando..." },
        ["Update_AlreadyChecking"] = new[]
        {
            "Já existe uma verificação em andamento. Tente de novo em instantes.",
            "A check is already in progress. Try again in a moment.",
            "Ya hay una búsqueda en curso. Inténtalo de nuevo en un momento.",
        },
        ["Update_AvailableSeeSidebar"] = new[]
        {
            "Nova versão disponível: {0}. Veja o aviso na lateral.",
            "New version available: {0}. See the notice on the side.",
            "Nueva versión disponible: {0}. Mira el aviso en el lateral.",
        },
        ["Update_UpToDate"] = new[] { "Você está na versão mais recente ({0}).", "You are on the latest version ({0}).", "Tienes la versión más reciente ({0})." },
        ["Update_CheckFailed"] = new[] { "Não foi possível verificar: {0}", "Could not check: {0}", "No se pudo buscar: {0}" },
        ["Update_ReadyTitle"] = new[] { "Pronto para instalar", "Ready to install", "Lista para instalar" },
        ["Update_ReadyText"] = new[] { "{0} já foi baixada. O LANShare vai reiniciar.", "{0} has been downloaded. LANShare will restart.", "{0} ya se descargó. LANShare se reiniciará." },
        ["Update_AvailableText"] = new[] { "{0} está disponível. Você usa {1}.", "{0} is available. You are using {1}.", "{0} está disponible. Usas {1}." },
        ["Update_Blocked"] = new[] { "Feche a sala ou desconecte para atualizar.", "Close the room or disconnect to update.", "Cierra la sala o desconéctate para actualizar." },
        ["Update_OnlyPublished"] = new[]
        {
            "A instalação automática só funciona no LANShare.exe publicado. Abrindo a página da versão.",
            "Automatic install only works with the published LANShare.exe. Opening the release page.",
            "La instalación automática solo funciona con el LANShare.exe publicado. Abriendo la página de la versión.",
        },
        ["Update_Downloading"] = new[] { "Baixando {0}...", "Downloading {0}...", "Descargando {0}..." },
        ["Update_StartingDownload"] = new[] { "Iniciando o download...", "Starting the download...", "Iniciando la descarga..." },
        ["Update_ProgressOf"] = new[] { "{0} de {1} ({2}%)", "{0} of {1} ({2}%)", "{0} de {1} ({2}%)" },
        ["Update_Downloaded"] = new[] { "{0} baixados", "{0} downloaded", "{0} descargados" },
        ["Update_DownloadFailed"] = new[] { "Falha no download: {0}", "Download failed: {0}", "Fallo en la descarga: {0}" },
        ["Update_Installing"] = new[] { "Instalando...", "Installing...", "Instalando..." },
        ["Update_WillRestart"] = new[] { "O LANShare vai reiniciar.", "LANShare will restart.", "LANShare se reiniciará." },
        ["Update_InstallFailed"] = new[] { "Falha ao instalar: {0}", "Install failed: {0}", "Fallo al instalar: {0}" },

        ["UpdateErr_NoStable"] = new[] { "Nenhuma versão estável publicada no GitHub ainda.", "No stable version published on GitHub yet.", "Todavía no hay ninguna versión estable publicada en GitHub." },
        ["UpdateErr_RateLimit"] = new[] { "Limite de consultas ao GitHub atingido. Tente mais tarde.", "GitHub request limit reached. Try again later.", "Se alcanzó el límite de consultas a GitHub. Inténtalo más tarde." },
        ["UpdateErr_HttpStatus"] = new[] { "O GitHub respondeu com erro {0}.", "GitHub replied with error {0}.", "GitHub respondió con el error {0}." },
        ["UpdateErr_BadTag"] = new[] { "A última versão do GitHub tem uma tag inválida.", "The latest GitHub release has an invalid tag.", "La última versión de GitHub tiene una etiqueta no válida." },
        ["UpdateErr_Timeout"] = new[] { "O GitHub demorou demais para responder.", "GitHub took too long to respond.", "GitHub tardó demasiado en responder." },
        ["UpdateErr_NoConnection"] = new[] { "Sem conexão com o GitHub.", "No connection to GitHub.", "Sin conexión con GitHub." },
        ["UpdateErr_BadResponse"] = new[] { "Resposta inválida do GitHub.", "Invalid response from GitHub.", "Respuesta no válida de GitHub." },
        ["UpdateErr_NoAsset"] = new[] { "Esta versão não tem o LANShare.exe para baixar.", "This version has no LANShare.exe to download.", "Esta versión no tiene el LANShare.exe para descargar." },
        ["UpdateErr_ExeNotFound"] = new[] { "Não foi possível localizar o LANShare.exe.", "Could not find LANShare.exe.", "No se pudo encontrar LANShare.exe." },
        ["UpdateErr_NoPermissionReplace"] = new[] { "Sem permissão para substituir o LANShare.exe nesta pasta.", "No permission to replace LANShare.exe in this folder.", "Sin permiso para reemplazar LANShare.exe en esta carpeta." },
        ["UpdateErr_ReplaceFailed"] = new[] { "Não foi possível substituir o LANShare.exe: {0}", "Could not replace LANShare.exe: {0}", "No se pudo reemplazar LANShare.exe: {0}" },
        ["UpdateErr_NotStarted"] = new[]
        {
            "A versão nova foi instalada, mas não abriu sozinha. Abra o LANShare de novo.",
            "The new version was installed but did not start on its own. Open LANShare again.",
            "La nueva versión se instaló, pero no se abrió sola. Abre LANShare de nuevo.",
        },
        ["UpdateErr_Incomplete"] = new[] { "O arquivo baixado está incompleto.", "The downloaded file is incomplete.", "El archivo descargado está incompleto." },
        ["UpdateErr_HashMismatch"] = new[] { "O arquivo baixado está corrompido (hash diferente).", "The downloaded file is corrupted (hash mismatch).", "El archivo descargado está dañado (hash distinto)." },
        ["UpdateErr_NotExe"] = new[] { "O arquivo baixado não é um programa válido.", "The downloaded file is not a valid program.", "El archivo descargado no es un programa válido." },
        ["UpdateErr_DownloadTimeout"] = new[] { "O download demorou demais.", "The download took too long.", "La descarga tardó demasiado." },
        ["UpdateErr_NoPermissionWrite"] = new[] { "Sem permissão para gravar na pasta do LANShare.", "No permission to write to the LANShare folder.", "Sin permiso para escribir en la carpeta de LANShare." },
        ["UpdateErr_SaveFailed"] = new[] { "Erro ao salvar o arquivo: {0}", "Error saving the file: {0}", "Error al guardar el archivo: {0}" },
    };
}

/// Texto traduzido dentro do XAML: Text="{l:Tr Settings_Title}".
/// Lido uma vez ao montar a tela (por isso trocar o idioma pede reinício).
public sealed class TrExtension : MarkupExtension
{
    public TrExtension() { }

    public TrExtension(string key) => Key = key;

    public string Key { get; set; } = "";

    public override object ProvideValue(IServiceProvider serviceProvider) => Loc.T(Key);
}

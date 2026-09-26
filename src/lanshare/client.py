import socket
import threading
import time

PORT = 5555

connected = True
host_username = "Host"
my_username = "Client"
last_seen = time.time()

HEARTBEAT_INTERVAL = 5   # segundos entre cada "sinal de vida"
TIMEOUT = 12              # segundos sem sinal = considera conexão perdida


def receive_messages(sock):
    global connected, host_username, last_seen
    while connected:
        try:
            data = sock.recv(1024)
            if not data:
                print("\n[STATUS] O Host desconectou.")
                connected = False
                break
            text = data.decode("utf-8")
            last_seen = time.time()
            if text == "__PING__":
                continue  # só um sinal de vida, não é mensagem de chat
            if text == "__DISCONNECT__":
                print(f"\n[STATUS] {host_username} saiu da rede.")
                connected = False
                break
            print(f"\n{host_username}: {text}")
        except (ConnectionResetError, OSError):
            print("\n[STATUS] Conexão perdida com o Host.")
            connected = False
            break


def send_heartbeat(sock):
    global connected
    while connected:
        try:
            time.sleep(HEARTBEAT_INTERVAL)
            if connected:
                sock.sendall("__PING__".encode("utf-8"))
        except OSError:
            break


def watch_connection():
    global connected
    while connected:
        time.sleep(1)
        if connected and (time.time() - last_seen) > TIMEOUT:
            print("\n[STATUS] Conexão perdida (sem resposta do Host).")
            connected = False
            break


def start_client():
    global connected, host_username, my_username, last_seen

    host_ip = input("Digite o IP do Host: ").strip()
    my_username = input("Digite seu nome de usuário: ").strip() or "Client"

    client_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)

    print("[STATUS] Conectando...")
    try:
        client_socket.connect((host_ip, PORT))
        print(f"[STATUS] Conectado ao Host {host_ip}:{PORT}", flush=True)
    except ConnectionRefusedError:
        print("[STATUS] Falha ao conectar. O Host está rodando e o IP está correto?")
        return

    client_socket.sendall(my_username.encode("utf-8"))
    host_username = client_socket.recv(1024).decode("utf-8")
    last_seen = time.time()
    print(f"[STATUS] Conectado com: {host_username}")
    print("Para sair da rede, digite 'sair'\n")

    threading.Thread(target=receive_messages, args=(client_socket,), daemon=True).start()
    threading.Thread(target=send_heartbeat, args=(client_socket,), daemon=True).start()
    threading.Thread(target=watch_connection, daemon=True).start()

    try:
        while connected:
            msg = input()
            if not connected:
                break
            print("\033[F\033[K" + f"{my_username}: {msg}")
            if msg.lower() == "sair":
                client_socket.sendall("__DISCONNECT__".encode("utf-8"))
                break
            client_socket.sendall(msg.encode("utf-8"))
    except (ConnectionResetError, OSError):
        pass
    finally:
        connected = False
        client_socket.close()
        print("[STATUS] Desconectado.")


if __name__ == "__main__":
    start_client()
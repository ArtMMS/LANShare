import socket
import threading
import time

HOST = "0.0.0.0"
PORT = 5555

connected = True
client_username = "Client"
my_username = "Host"
last_seen = time.time()

HEARTBEAT_INTERVAL = 5
TIMEOUT = 12


def receive_messages(conn):
    global connected, client_username, last_seen
    while connected:
        try:
            data = conn.recv(1024)
            if not data:
                print("\n[STATUS] Cliente desconectou.")
                connected = False
                break
            text = data.decode("utf-8")
            last_seen = time.time()
            if text == "__PING__":
                continue
            if text == "__DISCONNECT__":
                print(f"\n[STATUS] {client_username} saiu da rede.")
                connected = False
                break
            print(f"\n{client_username}: {text}")
        except (ConnectionResetError, OSError):
            print("\n[STATUS] Conexão perdida com o cliente.")
            connected = False
            break


def send_heartbeat(conn):
    global connected
    while connected:
        try:
            time.sleep(HEARTBEAT_INTERVAL)
            if connected:
                conn.sendall("__PING__".encode("utf-8"))
        except OSError:
            break


def watch_connection():
    global connected
    while connected:
        time.sleep(1)
        if connected and (time.time() - last_seen) > TIMEOUT:
            print("\n[STATUS] Conexão perdida (sem resposta do Cliente).")
            connected = False
            break


def start_host():
    global connected, client_username, my_username, last_seen

    my_username = input("Digite seu nome de usuário: ").strip() or "Host"

    server_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server_socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server_socket.bind((HOST, PORT))
    server_socket.listen(1)

    print(f"[STATUS] Aguardando conexão na porta {PORT}...", flush=True)

    conn, addr = server_socket.accept()
    print(f"[STATUS] Cliente conectado: {addr}")

    client_username = conn.recv(1024).decode("utf-8")
    conn.sendall(my_username.encode("utf-8"))
    last_seen = time.time()
    print(f"[STATUS] Conectado com: {client_username}")
    print("Para sair da rede, digite 'sair'\n")

    threading.Thread(target=receive_messages, args=(conn,), daemon=True).start()
    threading.Thread(target=send_heartbeat, args=(conn,), daemon=True).start()
    threading.Thread(target=watch_connection, daemon=True).start()

    try:
        while connected:
            msg = input()
            if not connected:
                break
            print("\033[F\033[K" + f"{my_username}: {msg}")
            if msg.lower() == "sair":
                conn.sendall("__DISCONNECT__".encode("utf-8"))
                break
            conn.sendall(msg.encode("utf-8"))
    except (ConnectionResetError, OSError):
        pass
    finally:
        connected = False
        conn.close()
        server_socket.close()
        print("[STATUS] Servidor encerrado.")


if __name__ == "__main__":
    start_host()
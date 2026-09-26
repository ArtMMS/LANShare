import socket
import threading

HOST = "0.0.0.0"
PORT = 5555

connected = True
client_username = "Client"
my_username = "Host"


def receive_messages(conn):
    global connected, client_username
    while connected:
        try:
            data = conn.recv(1024)
            if not data:
                print("\n[HOST] Cliente desconectou.")
                connected = False
                break
            text = data.decode("utf-8")
            if text == "__DISCONNECT__":
                print(f"\n[HOST] {client_username} saiu da rede.")
                connected = False
                break
            print(f"\n{client_username}: {text}")
        except (ConnectionResetError, OSError):
            print("\n[HOST] Conexão perdida com o cliente.")
            connected = False
            break


def start_host():
    global connected, client_username, my_username

    my_username = input("Digite seu nome de usuário: ").strip() or "Host"

    server_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server_socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server_socket.bind((HOST, PORT))
    server_socket.listen(1)

    print(f"[HOST] Aguardando conexão na porta {PORT}...", flush=True)

    conn, addr = server_socket.accept()
    print(f"[HOST] Cliente conectado: {addr}")

    client_username = conn.recv(1024).decode("utf-8")
    conn.sendall(my_username.encode("utf-8"))
    print(f"[HOST] Conectado com: {client_username}")
    print("Para sair da rede, digite 'sair'\n")

    receiver_thread = threading.Thread(target=receive_messages, args=(conn,), daemon=True)
    receiver_thread.start()

    try:
        while connected:
            msg = input()
            if not connected:
                break
            # apaga a linha que o terminal ecoou e reescreve formatada
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
        print("[HOST] Servidor encerrado.")


if __name__ == "__main__":
    start_host()
import socket
import threading

PORT = 5555

connected = True
host_username = "Host"
my_username = "Client"


def receive_messages(sock):
    global connected, host_username
    while connected:
        try:
            data = sock.recv(1024)
            if not data:
                print("\n[CLIENT] O Host desconectou.")
                connected = False
                break
            text = data.decode("utf-8")
            if text == "__DISCONNECT__":
                print(f"\n[CLIENT] {host_username} saiu da rede.")
                connected = False
                break
            print(f"\n{host_username}: {text}")
        except (ConnectionResetError, OSError):
            print("\n[CLIENT] Conexão perdida com o Host.")
            connected = False
            break


def start_client():
    global connected, host_username, my_username

    host_ip = input("Digite o IP do Host: ").strip()
    my_username = input("Digite seu nome de usuário: ").strip() or "Client"

    client_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)

    try:
        client_socket.connect((host_ip, PORT))
        print(f"[CLIENT] Conectado ao Host {host_ip}:{PORT}", flush=True)
    except ConnectionRefusedError:
        print("[CLIENT] Não foi possível conectar. O Host está rodando e o IP está correto?")
        return

    client_socket.sendall(my_username.encode("utf-8"))
    host_username = client_socket.recv(1024).decode("utf-8")
    print(f"[CLIENT] Conectado com: {host_username}")
    print("Para sair da rede, digite 'sair'\n")

    receiver_thread = threading.Thread(target=receive_messages, args=(client_socket,), daemon=True)
    receiver_thread.start()

    try:
        while connected:
            msg = input()
            if not connected:
                break
            # apaga a linha que o terminal ecoou e reescreve formatada
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
        print("[CLIENT] Desconectado.")


if __name__ == "__main__":
    start_client()
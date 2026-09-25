import socket

PORT = 5555  # tem que ser a mesma porta usada no host.py


def start_client():
    host_ip = input("Digite o IP do Host: ").strip()

    client_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)

    try:
        client_socket.connect((host_ip, PORT))
        print(f"[CLIENT] Conectado ao Host {host_ip}:{PORT}", flush=True)

        welcome = client_socket.recv(1024)
        print(f"[CLIENT] Mensagem do Host: {welcome.decode('utf-8')}")

        while True:
            msg = input("Digite uma mensagem para o Host (ou 'sair' para desconectar): ")
            if msg.lower() == "sair":
                break
            client_socket.sendall(msg.encode("utf-8"))

    except ConnectionRefusedError:
        print("[CLIENT] Não foi possível conectar. O Host está rodando e o IP está correto?")
    except ConnectionResetError:
        print("[CLIENT] Conexão perdida com o Host.")
    finally:
        client_socket.close()
        print("[CLIENT] Desconectado.")


if __name__ == "__main__":
    start_client()
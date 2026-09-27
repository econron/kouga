"""Local SMTP-over-TLS sink for the opt-in mailer integration test.

Run two instances with distinct certificates: one issued by the temporary test
CA and one self-signed. Default bind is loopback; an isolated Docker bridge
can use --bind 0.0.0.0 without publishing a host port. The script has no
outbound connection or delivery path.
"""

import argparse
import base64
import socket
import ssl
import threading


def serve_connection(raw, context):
    try:
        with context.wrap_socket(raw, server_side=True) as secure:
            stream = secure.makefile("rwb", buffering=0)
            stream.write(b"220 localhost test SMTP\r\n")
            authenticated = False
            data = False
            while line := stream.readline():
                if data:
                    if line == b".\r\n":
                        stream.write(b"250 accepted\r\n")
                        print("DATA accepted", flush=True)
                        data = False
                    continue
                command = line.decode("ascii", "replace").strip()
                if command.startswith("EHLO"):
                    stream.write(b"250-localhost\r\n250 AUTH PLAIN\r\n")
                elif command.startswith("AUTH PLAIN"):
                    token = command.removeprefix("AUTH PLAIN").strip()
                    if not token:
                        stream.write(b"334 \r\n")
                        token = stream.readline().decode("ascii", "replace").strip()
                    try:
                        valid = base64.b64decode(token, validate=True) == b"\x00local-user\x00local-pass"
                    except ValueError:
                        valid = False
                    authenticated = valid
                    stream.write(b"235 authenticated\r\n" if valid else b"535 rejected\r\n")
                    print("AUTH accepted" if valid else "AUTH rejected", flush=True)
                elif command.startswith("MAIL FROM:") or command.startswith("RCPT TO:"):
                    stream.write(b"250 OK\r\n" if authenticated else b"530 authenticate first\r\n")
                elif command == "DATA":
                    if authenticated:
                        data = True
                        stream.write(b"354 end with dot\r\n")
                    else:
                        stream.write(b"530 authenticate first\r\n")
                elif command == "QUIT":
                    stream.write(b"221 bye\r\n")
                    return
                else:
                    stream.write(b"502 unsupported\r\n")
    except ssl.SSLError:
        print("TLS rejected", flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--bind", default="127.0.0.1")
    parser.add_argument("--cert", required=True)
    parser.add_argument("--key", required=True)
    args = parser.parse_args()
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(args.cert, args.key)
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind((args.bind, args.port))
        listener.listen(4)
        print(f"listening on {args.port}", flush=True)
        try:
            while True:
                raw, _ = listener.accept()
                threading.Thread(target=serve_connection, args=(raw, context), daemon=True).start()
        except KeyboardInterrupt:
            return


if __name__ == "__main__":
    main()

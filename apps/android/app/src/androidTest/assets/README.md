`self-signed.p12` is a throwaway self-signed certificate for `127.0.0.1` (password `test-only`),
served by `PlatformTlsDeviceTest` to check that the core refuses a certificate no authority
signed. It protects nothing. To make it again:

```sh
openssl req -x509 -newkey rsa:2048 -nodes -keyout key.pem -out cert.pem -days 3650 \
  -subj "/CN=localhost" -addext "subjectAltName=IP:127.0.0.1,DNS:localhost"
openssl pkcs12 -export -inkey key.pem -in cert.pem -out self-signed.p12 -passout pass:test-only \
  -name self-signed -keypbe AES-256-CBC -certpbe AES-256-CBC -macalg sha256
```

These certificates and the server private key are test fixtures, generated for
local HTTPS tests. They must never be used for production services or installed
in a system trust store. The server certificate covers `localhost` only.

`ca.pem` signs the server certificate; `other-ca.pem` is an unrelated CA used to
check that certificate verification rejects an untrusted issuer. Certificates
have long validity periods so the tests do not expire during normal maintenance.

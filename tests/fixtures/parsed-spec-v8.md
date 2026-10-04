# Prior parsed-spec layout fixture

`parsed-spec-v8.bin` was generated with the landed spec slice's layout-8 CLI,
which stores grouped security requirements but has no parameter-serialization
fields. It is a genuine prior layout, not a current model with its version edited.

Source: the synthetic `parsed-spec-v8.yaml` compatibility probe
(OpenAPI 3.0.3, `/users/{id}`, path `id` and query `q` referencing a string
component, server `http://127.0.0.1:9`). Generated in a disposable isolated
`APERTURE_CONFIG_DIR` using `aperture config add prior8 parsed-spec-v8.yaml`; no network
request was made. The fixture contains no credentials.

SHA-256: `3d71e46f6d891180e7e3dc4f7b95c596b7688300218c36f980d8ae5fdc717b13`.

The loader regression rejects it with layout-8 metadata and with current metadata
that would otherwise allow its incompatible embedded fields to be interpreted.

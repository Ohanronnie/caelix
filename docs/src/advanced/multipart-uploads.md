# Multipart Uploads

Caelix accepts file uploads alongside typed request DTOs on both supported
runtimes. A controller method keeps its normal `#[body]` DTO and declares file
parts independently with `#[file]` or `#[files]`. JSON clients and multipart
clients can therefore use the same route whenever its file arguments are
optional.

## Typed fields and one required file

Use `#[body]` for the non-file fields and `#[file]` for one upload. Multipart
text fields use typed Serde form decoding: strings, numbers, booleans, optional
fields, and repeated collection fields are decoded into the DTO before the
controller runs.

```rust
use caelix::{Response, Result, UploadedFile, controller, injectable};
use serde::Deserialize;
use serde_json::{Value, json};
use validator::Validate;

#[derive(Deserialize, Validate)]
struct DocumentInput {
    #[validate(length(min = 3))]
    title: String,
    #[validate(range(min = 1))]
    quantity: u32,
    published: bool,
    #[serde(default)]
    labels: Vec<String>,
}

#[injectable]
struct DocumentsController;

#[controller("/documents")]
impl DocumentsController {
    #[post("")]
    async fn create(
        &self,
        #[body] #[validate] input: DocumentInput,
        #[file] document: UploadedFile,
    ) -> Result<Response<Value>> {
        let contents = document.read_bytes().await?;
        Ok(Response::Body(json!({
            "title": input.title,
            "bytes": contents.len(),
        })))
    }
}
```

The file-part name defaults to the Rust parameter name. Rename it when the HTTP
contract needs a different name:

```rust
#[file(name = "cover_image")] image: UploadedFile
```

## File validation

`#[file(...)]` and `#[files(...)]` can validate each staged file before the
handler runs. `max_size` accepts a whole number with `B`, `KB`, `MB`, `GB`,
`KiB`, `MiB`, or `GiB`; decimal suffixes use powers of 1,000 and IEC suffixes
use powers of 1,024. `content_type` is a comma-separated MIME allowlist.

```rust
#[post("/documents")]
async fn create(
    &self,
    #[file(
        name = "document",
        max_size = "10MiB",
        content_type = "application/pdf, image/png",
        validate = validate_document,
    )]
    document: UploadedFile,
) -> Result<Response<Document>> {
    Ok(Response::Body(self.documents.store(document).await?))
}
```

Caelix checks, in order: declared size, declared content type, then the
controller validator. Size violations return `413 Payload Too Large`; MIME
violations and undetectable files return `400 Bad Request`. MIME validation is
secure by default: Caelix detects the type from the file's magic bytes and does
not trust a client filename or part header.

Set `trust_content_type_header = true` only when that header is part of your
explicit trust boundary. It must accompany `content_type`; header parameters
such as `charset=utf-8` are ignored. This mode accepts the client declaration
instead of inspecting file bytes, so it is unsafe for untrusted clients.

Validators must be controller methods with this exact shape:

```rust
async fn validate_document(&self, file: &UploadedFile) -> Result<()> {
    self.document_rules.validate(file).await
}
```

For reusable rules, inject a service into the controller and forward to it as
above. A validator receives only its attached file; keep cross-field checks in
the handler body after every extractor is available. Optional files skip all
file checks when absent. Repeated files are checked in request order. If any
check fails, Caelix drops the collected files and all remaining staged files,
deleting their temporary paths before returning the error response.

`#[validate]` runs after JSON or multipart DTO decoding. A successful multipart
decode that fails validation returns Caelix's normal `400 Bad Request` validation
envelope, including its `errors` map.

## Content negotiation

`#[body]` accepts the following content types:

| Route arguments | Accepted request content types | File argument result |
| --- | --- | --- |
| DTO only | `application/json`, omitted content type, `multipart/form-data` | Not applicable |
| DTO + required `UploadedFile` | `multipart/form-data` | Required file must occur exactly once |
| DTO + `Option<UploadedFile>` | JSON, omitted content type, `multipart/form-data` | JSON supplies `None`; multipart may supply one file |
| DTO + `Vec<UploadedFile>` | `multipart/form-data` | Repeated file field, or an empty vector when absent |
| `#[multipart] MultipartForm` | `multipart/form-data` | Full form access |

Other declared content types return `415 Unsupported Media Type`. Invalid
boundaries, malformed parts, duplicate values for a single-file extractor, and
invalid typed form values return `400 Bad Request`.

## Optional and repeated files

Optional files allow the endpoint to retain a JSON workflow. They are `None`
for JSON requests and when the named multipart part is not present:

```rust
#[post("/profile")]
async fn update(
    &self,
    #[body] input: UpdateProfile,
    #[file(name = "avatar")] avatar: Option<UploadedFile>,
) -> Result<Response<Profile>> {
    if let Some(avatar) = avatar {
        self.profiles.replace_avatar(avatar).await?;
    }
    Ok(Response::Body(self.profiles.update(input).await?))
}
```

Use `Vec<UploadedFile>` for repeated parts under one field name:

```rust
#[post("/attachments")]
async fn attach(
    &self,
    #[files(name = "attachment")] attachments: Vec<UploadedFile>,
) -> Result<Response<AttachmentSummary>> {
    // `attachments` is empty when the field is absent.
    Ok(Response::Body(self.files.store_all(attachments).await?))
}
```

## Full-form access

Use `#[multipart] MultipartForm` when the controller needs direct control over
repeated text or file fields instead of binding a DTO. It cannot be combined
with `#[body]`, `#[file]`, or `#[files]` on the same route.

```rust
#[post("/import")]
async fn import(&self, #[multipart] mut form: MultipartForm) -> Result<Response<ImportResult>> {
    let mode = form.text("mode").and_then(|values| values.first());
    let files = form.take_files("source");
    Ok(Response::Body(self.importer.run(mode, files).await?))
}
```

`text(name)` and `files(name)` borrow the form. `take_file(name)` consumes one
file and returns `400 Bad Request` if that field has duplicates; `take_files`
consumes every file with that name.

## Uploaded file lifecycle

`UploadedFile` stages each file in Caelix's isolated temporary directory. The
handle exposes its multipart field name, client filename, optional content type,
headers, byte size, and temporary path.

```rust
let bytes = upload.read_bytes().await?;
let destination = upload.persist_to("/srv/documents/report.pdf").await?;
```

These operations must happen in the opposite order when both are required:
`persist_to` consumes the upload handle, so call `read_bytes` first. Temporary
files are removed when their `UploadedFile` is dropped. `persist_to` requires
the destination directory to exist and uses no-overwrite creation; it returns an
error rather than replacing an existing file. Caelix supplies temporary upload
handling only—applications choose permanent local, object-storage, or database
storage themselves.

## Limits and temporary storage

All multipart requests use `Application::body_limit`, which defaults to 1 MiB.
Configure a larger application limit and an application-owned staging directory
before `listen`:

```rust
Application::new::<AppModule>()
    .await?
    .body_limit(10 * 1024 * 1024)
    .upload_temp_dir("/var/tmp/my-service/uploads")
    .listen("127.0.0.1:3000")
    .await?;
```

Apply a stricter multipart limit to one route with `#[upload(limit = ...)]`:

```rust
#[upload(limit = 512 * 1024)]
#[post("/avatar")]
async fn avatar(&self, #[file] image: UploadedFile) -> Result<Response<Avatar>> {
    // ...
}
```

An overflow returns `413 Payload Too Large` and includes the effective limit in
the response message. Storage failures are logged with their details but expose
only Caelix's standard internal-server-error response to clients. `TestApplication`
offers the same `.body_limit(...)` and `.upload_temp_dir(...)` configuration.

Upload support is enabled by default. Applications that opt out with
`default-features = false` can omit the multipart extractor APIs and their
multipart dependencies entirely.

## Testing with curl

This request exercises typed DTO binding, repeated values, file metadata, and
the normal validation path:

```sh
curl --fail-with-body \
  -F 'title=Quarterly report' \
  -F 'quantity=42' \
  -F 'published=true' \
  -F 'labels=rust' \
  -F 'labels=uploads' \
  -F 'document=@report.pdf;type=application/pdf' \
  http://127.0.0.1:3000/documents
```

Use raw multipart payloads through `TestApplication::set_payload(...)` when an
integration test needs malformed-boundary or duplicate-file coverage. The
[Testing recipe](../recipes/testing.md) shows the in-process request helpers.

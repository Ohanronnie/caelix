# Error Response Shape

HTTP exceptions serialize as JSON:

```json
{
  "status": 404,
  "error": "Not Found",
  "message": "user not found"
}
```

Validation or structured client errors can include `errors`:

```json
{
  "status": 422,
  "error": "Unprocessable Entity",
  "message": "invalid request",
  "errors": {
    "email": ["must be an email"]
  }
}
```

For 5xx statuses, Caelix hides the internal message from the response body:

```json
{
  "status": 500,
  "error": "Internal Server Error",
  "message": "Internal Server Error"
}
```

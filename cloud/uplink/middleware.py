"""Client certificate extraction for any deployment target.

Checks multiple sources so the same code works behind Lambda Web Adapter,
an nginx ingress, or with no TLS termination (local dev).
"""

import json

from starlette.requests import Request


def extract_client_subject(request: Request) -> str | None:
    """Extract mTLS client certificate subject DN from available sources.

    Checks (in order):
    1. x-amzn-request-context header (Lambda Web Adapter / API Gateway)
    2. x-ssl-client-subject-dn header (nginx ingress / reverse proxy)
    3. None (local dev, no mTLS)
    """
    # Lambda Web Adapter passes API Gateway requestContext as a header
    ctx_header = request.headers.get("x-amzn-request-context")
    if ctx_header:
        try:
            ctx = json.loads(ctx_header)
            return ctx["identity"]["clientCert"]["subjectDN"]
        except (json.JSONDecodeError, KeyError, TypeError):
            pass

    # Reverse proxy / ingress controller convention
    ssl_subject = request.headers.get("x-ssl-client-subject-dn")
    if ssl_subject:
        return ssl_subject

    return None

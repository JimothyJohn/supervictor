"""Starlette ASGI application for the Supervictor uplink API.

Thin HTTP adapter — routes requests to handlers, serializes responses.
Runs on Lambda (via Web Adapter), Fargate, K8s, or docker run.
"""

import json

from pydantic import BaseModel
from starlette.applications import Starlette
from starlette.requests import Request
from starlette.responses import JSONResponse
from starlette.routing import Route

from uplink.handlers import handle_hello, handle_uplink
from uplink.middleware import extract_client_subject


async def hello(request: Request) -> JSONResponse:
    client_subject = extract_client_subject(request)
    result = handle_hello(client_subject=client_subject)
    return JSONResponse(
        json.loads(result.model_dump_json(exclude_none=True)),
        status_code=200,
    )


async def uplink(request: Request) -> JSONResponse:
    client_subject = extract_client_subject(request)
    raw_body = (await request.body()).decode()
    result, status = handle_uplink(raw_body, client_subject=client_subject)
    if isinstance(result, BaseModel):
        body = json.loads(result.model_dump_json(exclude_none=True))
    else:
        body = result
    return JSONResponse(body, status_code=status)


app = Starlette(
    routes=[
        Route("/", hello, methods=["GET"]),
        Route("/", uplink, methods=["POST"]),
    ],
)

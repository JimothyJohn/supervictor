"""Starlette ASGI application for the Supervictor uplink API.

Thin HTTP adapter — routes requests to handlers, serializes responses.
Runs on Lambda (via Web Adapter), Fargate, K8s, or docker run.
"""

import json
import logging

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(name)s %(levelname)s %(message)s")

from pydantic import BaseModel
from starlette.applications import Starlette
from starlette.requests import Request
from starlette.responses import JSONResponse
from starlette.routing import Route
from uplink.handlers import (
    handle_get_device,
    handle_get_device_uplinks,
    handle_hello,
    handle_list_devices,
    handle_register_device,
    handle_uplink,
)
from uplink.middleware import extract_client_subject
from uplink.store_factory import create_store

store = create_store()


def _serialize(result: object) -> object:
    """Convert a Pydantic model or list of models to JSON-safe dicts."""
    if isinstance(result, list):
        return [json.loads(r.model_dump_json(exclude_none=True)) for r in result]
    if isinstance(result, BaseModel):
        return json.loads(result.model_dump_json(exclude_none=True))
    return result


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
    result, status = handle_uplink(raw_body, client_subject=client_subject, store=store)
    return JSONResponse(_serialize(result), status_code=status)


async def register_device(request: Request) -> JSONResponse:
    raw_body = (await request.body()).decode()
    result, status = handle_register_device(raw_body, store=store)
    return JSONResponse(_serialize(result), status_code=status)


async def list_devices(request: Request) -> JSONResponse:
    result, status = handle_list_devices(store=store)
    return JSONResponse(_serialize(result), status_code=status)


async def get_device(request: Request) -> JSONResponse:
    device_id = request.path_params["device_id"]
    result, status = handle_get_device(device_id, store=store)
    return JSONResponse(_serialize(result), status_code=status)


async def get_device_uplinks(request: Request) -> JSONResponse:
    device_id = request.path_params["device_id"]
    result, status = handle_get_device_uplinks(device_id, store=store)
    return JSONResponse(result, status_code=status)


async def health(request: Request) -> JSONResponse:
    return JSONResponse({"status": "ok"}, status_code=200)


app = Starlette(
    routes=[
        Route("/health", health, methods=["GET"]),
        Route("/", hello, methods=["GET"]),
        Route("/", uplink, methods=["POST"]),
        Route("/devices", register_device, methods=["POST"]),
        Route("/devices", list_devices, methods=["GET"]),
        Route("/devices/{device_id:str}", get_device, methods=["GET"]),
        Route("/devices/{device_id:str}/uplinks", get_device_uplinks, methods=["GET"]),
    ],
)

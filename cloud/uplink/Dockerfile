# --- Build stage: install dependencies with uv ---
FROM python:3.13-slim AS build

COPY --from=ghcr.io/astral-sh/uv:0.7 /uv /usr/local/bin/uv

WORKDIR /app
COPY pyproject.toml ./

# Install deps only (cached until pyproject.toml changes)
RUN uv venv .venv && \
    python3 -c "import tomllib; print('\n'.join(tomllib.load(open('pyproject.toml','rb'))['project']['dependencies']))" | \
    uv pip install --python .venv/bin/python --no-cache -r -

COPY *.py uplink/

# --- Runtime stage: slim image with Lambda Web Adapter ---
FROM python:3.13-slim

COPY --from=public.ecr.aws/awsguru/aws-lambda-adapter:0.8.4 \
     /lambda-adapter /opt/extensions/lambda-adapter

WORKDIR /app
COPY --from=build /app/.venv /app/.venv
COPY --from=build /app/uplink ./uplink

ENV PATH="/app/.venv/bin:$PATH"
ENV PYTHONUNBUFFERED=1
ENV PORT=8000

EXPOSE 8000
CMD ["uvicorn", "uplink.app:app", "--host", "0.0.0.0", "--port", "8000"]

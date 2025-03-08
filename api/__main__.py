from fastapi import FastAPI, Request
from pydantic import BaseModel
import uvicorn
import json

app = FastAPI()


class Item(BaseModel):
    description: str | None = None
    price: float
    tax: float | None = None


@app.get("/")
async def root():
    return {"message": "Hello world"}


@app.post("/post")
async def post_endpoint(request: Request):
    # AI-generated: Handle POST requests to /post endpoint
    # Extract the "test" key from the incoming JSON data
    # message_value = data.get("message", "No message key found")
    print(f"Received message: {await request.body()}")
    return {"message": "POST accepted!"}


if __name__ == "__main__":
    uvicorn.run(app, host="0.0.0.0", port=8000)

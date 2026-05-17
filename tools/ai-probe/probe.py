#!/usr/bin/env python3
"""GeulOS AI Probe — tests whether Claude can use GeulOS via the wire protocol.

Usage:
    python probe.py --scenario 02_press_button
    python probe.py --scenario 03_multi_press --model claude-sonnet-4-6
    python probe.py --scenario 01_list_all --server 127.0.0.1:5550

The probe connects to a running GeulOS server-host, sets up an anthropic API
agent loop, gives Claude four tool functions wrapping the wire protocol, and
records every turn to `results/<timestamp>_<scenario>.log`.
"""

from __future__ import annotations

import argparse
import json
import os
import socket
import struct
import sys
import time
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

try:
    from anthropic import Anthropic
    from dotenv import load_dotenv
except ImportError as e:
    print(f"missing dep: {e}\n  pip install -r requirements.txt", file=sys.stderr)
    sys.exit(1)


# --------------------------------------------------------------------------
# Paths
# --------------------------------------------------------------------------

SCRIPT_DIR = Path(__file__).parent.resolve()
WORKSPACE_ROOT = SCRIPT_DIR.parent.parent
RESULTS_DIR = SCRIPT_DIR / "results"
SCENARIOS_DIR = SCRIPT_DIR / "scenarios"
SYSTEM_PROMPT_PATH = SCRIPT_DIR / "system_prompt.md"


# --------------------------------------------------------------------------
# Wire protocol — 4-byte big-endian length prefix + JSON body
# --------------------------------------------------------------------------


def encode_frame(body: bytes) -> bytes:
    return struct.pack(">I", len(body)) + body


def read_exact(sock: socket.socket, n: int) -> bytes:
    buf = bytearray()
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise ConnectionError(f"socket closed (wanted {n} bytes, got {len(buf)})")
        buf.extend(chunk)
    return bytes(buf)


def read_frame(sock: socket.socket) -> dict[str, Any]:
    header = read_exact(sock, 4)
    length = struct.unpack(">I", header)[0]
    body = read_exact(sock, length)
    return json.loads(body)


class WireClient:
    def __init__(self, host: str, port: int):
        self.sock = socket.create_connection((host, port), timeout=10.0)
        hello = {
            "kind": "Hello",
            "version": "0.1",
            "role": "ai",
            "auth": {},
            "client_id": "ai-probe",
        }
        self.send(hello)
        ack = self.recv()
        if ack.get("kind") != "HelloAck":
            raise RuntimeError(f"expected HelloAck, got {ack}")
        self.actor_id: str = ack["actor_id"]
        self.session_id: str = ack["session_id"]

    def send(self, msg: dict[str, Any]) -> None:
        body = json.dumps(msg).encode("utf-8")
        self.sock.sendall(encode_frame(body))

    def recv(self) -> dict[str, Any]:
        return read_frame(self.sock)

    def request(self, msg: dict[str, Any]) -> dict[str, Any]:
        self.send(msg)
        return self.recv()

    def close(self) -> None:
        try:
            self.sock.close()
        except Exception:
            pass


# --------------------------------------------------------------------------
# Tool implementations — these are what Claude calls
# --------------------------------------------------------------------------


def tool_list_objects_by_type(wire: WireClient, type_uri: str) -> dict[str, Any]:
    msg = {
        "kind": "Query",
        "request_id": f"q-{uuid.uuid4()}",
        "query": {"ByType": {"type_uri": type_uri}},
    }
    resp = wire.request(msg)
    if resp.get("kind") == "QueryResult":
        return {"object_ids": resp.get("objects", [])}
    return {"error": "unexpected response", "raw": resp}


def tool_get_object(wire: WireClient, object_id: str) -> dict[str, Any]:
    msg = {
        "kind": "Get",
        "request_id": f"g-{uuid.uuid4()}",
        "target": object_id,
    }
    resp = wire.request(msg)
    if resp.get("kind") == "GetResult":
        return {"object": resp.get("object")}
    if resp.get("kind") == "GetError":
        return {
            "error": resp.get("error_kind", "unknown"),
            "detail": resp.get("detail", ""),
        }
    return {"error": "unexpected response", "raw": resp}


def tool_invoke_method(
    wire: WireClient, target: str, method: str, args: Any
) -> dict[str, Any]:
    msg = {
        "kind": "Invoke",
        "request_id": f"i-{uuid.uuid4()}",
        "target": target,
        "method": method,
        "args": args if args is not None else None,
    }
    resp = wire.request(msg)
    if resp.get("kind") == "InvokeAck":
        return {
            "ok": True,
            "event_id": resp.get("event_id"),
            "result": resp.get("result"),
        }
    if resp.get("kind") == "InvokeError":
        return {
            "ok": False,
            "error": resp.get("error_kind", "unknown"),
            "detail": resp.get("detail", ""),
        }
    return {"ok": False, "error": "unexpected response", "raw": resp}


# --------------------------------------------------------------------------
# Tool declarations for Claude
# --------------------------------------------------------------------------


TOOLS = [
    {
        "name": "list_objects_by_type",
        "description": (
            "List all object IDs that match a given type URI. "
            "Standard types: aios.std/Container@1, aios.std/Text@1, "
            "aios.std/Button@1, aios.std/Toggle@1."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "type_uri": {
                    "type": "string",
                    "description": "Type URI (e.g. aios.std/Button@1)",
                }
            },
            "required": ["type_uri"],
        },
    },
    {
        "name": "get_object",
        "description": (
            "Fetch the full details of an object by its UUID. "
            "Returns props, state, methods, parent, children, owner, ACL."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "object_id": {
                    "type": "string",
                    "description": "Object UUID (exact string from a previous query)",
                }
            },
            "required": ["object_id"],
        },
    },
    {
        "name": "invoke_method",
        "description": (
            "Invoke a method on an object. "
            "Returns event_id on success, or error_kind ('permission', "
            "'not_found', 'unknown_method', etc.) on failure."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "target": {"type": "string", "description": "Object UUID"},
                "method": {
                    "type": "string",
                    "description": "Method name from the object's methods list",
                },
                "args": {
                    "description": "JSON-serializable args, or null",
                },
            },
            "required": ["target", "method"],
        },
    },
    {
        "name": "report_done",
        "description": (
            "Call this exactly once at the end with a summary of what you found "
            "and what you did. After this the session ends."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "3-5 sentence summary, specific and honest",
                }
            },
            "required": ["summary"],
        },
    },
]


# --------------------------------------------------------------------------
# Agent loop
# --------------------------------------------------------------------------


def run_agent(
    client: Anthropic,
    wire: WireClient,
    system_prompt: str,
    scenario_text: str,
    model: str,
    max_turns: int,
    log: list[str],
) -> dict[str, Any]:
    messages: list[dict[str, Any]] = [
        {"role": "user", "content": scenario_text}
    ]
    final_summary: str | None = None
    total_input_tokens = 0
    total_output_tokens = 0

    for turn in range(1, max_turns + 1):
        log.append(f"\n--- Turn {turn} ---")
        response = client.messages.create(
            model=model,
            max_tokens=2048,
            system=system_prompt,
            tools=TOOLS,
            messages=messages,
        )
        total_input_tokens += response.usage.input_tokens
        total_output_tokens += response.usage.output_tokens

        # Capture any text blocks
        text_chunks = []
        tool_uses = []
        for block in response.content:
            if block.type == "text":
                text_chunks.append(block.text)
            elif block.type == "tool_use":
                tool_uses.append(block)

        if text_chunks:
            log.append("Claude (text):")
            for t in text_chunks:
                log.append(f"  {t}")

        # Add assistant turn
        messages.append({"role": "assistant", "content": response.content})

        if response.stop_reason == "end_turn" and not tool_uses:
            log.append("Claude stopped without calling report_done.")
            break

        # Execute each tool call, gather results for next user turn
        tool_results = []
        for tu in tool_uses:
            log.append(f"Tool call: {tu.name}({json.dumps(tu.input, ensure_ascii=False)})")
            result: Any
            try:
                if tu.name == "list_objects_by_type":
                    result = tool_list_objects_by_type(wire, tu.input["type_uri"])
                elif tu.name == "get_object":
                    result = tool_get_object(wire, tu.input["object_id"])
                elif tu.name == "invoke_method":
                    args = tu.input.get("args")
                    result = tool_invoke_method(
                        wire, tu.input["target"], tu.input["method"], args
                    )
                elif tu.name == "report_done":
                    final_summary = tu.input["summary"]
                    result = {"ok": True}
                else:
                    result = {"error": f"unknown tool {tu.name}"}
            except Exception as e:
                result = {"error": str(e)}

            result_repr = json.dumps(result, ensure_ascii=False)
            if len(result_repr) > 1000:
                result_repr = result_repr[:1000] + "...(truncated)"
            log.append(f"  -> {result_repr}")

            tool_results.append(
                {
                    "type": "tool_result",
                    "tool_use_id": tu.id,
                    "content": json.dumps(result, ensure_ascii=False),
                }
            )

        if final_summary is not None:
            break

        messages.append({"role": "user", "content": tool_results})

    return {
        "summary": final_summary,
        "turns": turn,
        "input_tokens": total_input_tokens,
        "output_tokens": total_output_tokens,
    }


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser()
    p.add_argument(
        "--scenario",
        required=True,
        help="Name of scenario file (e.g. 02_press_button) — looked up in scenarios/",
    )
    p.add_argument(
        "--server",
        default="127.0.0.1:5550",
        help="GeulOS server-host address (default 127.0.0.1:5550)",
    )
    p.add_argument(
        "--model",
        default="claude-sonnet-4-6",
        help="Anthropic model id (default claude-sonnet-4-6)",
    )
    p.add_argument(
        "--max-turns",
        type=int,
        default=12,
        help="Max agent turns before forced stop (default 12)",
    )
    return p.parse_args()


def main() -> int:
    args = parse_args()

    # Load .env from workspace root
    env_path = WORKSPACE_ROOT / ".env"
    if not env_path.exists():
        print(f"missing .env at {env_path}", file=sys.stderr)
        return 1
    load_dotenv(env_path)
    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        print("ANTHROPIC_API_KEY not set in .env", file=sys.stderr)
        return 1

    # Load scenario
    scenario_path = SCENARIOS_DIR / f"{args.scenario}.md"
    if not scenario_path.exists():
        print(f"missing scenario: {scenario_path}", file=sys.stderr)
        return 1
    scenario_text = scenario_path.read_text(encoding="utf-8").strip()

    # Load system prompt
    system_prompt = SYSTEM_PROMPT_PATH.read_text(encoding="utf-8")

    # Connect
    host, port_s = args.server.split(":")
    port = int(port_s)
    try:
        wire = WireClient(host, port)
    except (ConnectionRefusedError, OSError) as e:
        print(
            f"could not connect to GeulOS at {args.server}: {e}\n"
            f"  is server-host running? (cargo run -p geulos-server-host)",
            file=sys.stderr,
        )
        return 1

    client = Anthropic(api_key=api_key)

    log: list[str] = []
    started = datetime.now(timezone.utc)
    log.append(f"=== GeulOS AI Probe ===")
    log.append(f"Timestamp: {started.isoformat()}")
    log.append(f"Scenario: {args.scenario}")
    log.append(f"Server: {args.server}")
    log.append(f"Model: {args.model}")
    log.append(f"Actor (assigned): {wire.actor_id}")
    log.append(f"Session: {wire.session_id}")
    log.append(f"\n=== Scenario text ===\n{scenario_text}")
    log.append("\n=== Conversation ===")

    t_start = time.time()
    try:
        result = run_agent(
            client,
            wire,
            system_prompt,
            scenario_text,
            args.model,
            args.max_turns,
            log,
        )
    except Exception as e:
        log.append(f"\n!! agent crashed: {e}")
        result = {"summary": None, "turns": 0, "input_tokens": 0, "output_tokens": 0}
    elapsed = time.time() - t_start

    log.append("\n=== Final ===")
    log.append(f"Wall time: {elapsed:.1f}s")
    log.append(f"Turns: {result['turns']}")
    log.append(f"Tokens (in/out): {result['input_tokens']}/{result['output_tokens']}")
    if result["summary"]:
        log.append("Status: completed via report_done")
        log.append(f"Summary: {result['summary']}")
    else:
        log.append("Status: ended without report_done")

    # Persist
    RESULTS_DIR.mkdir(exist_ok=True)
    fname = (
        started.strftime("%Y%m%d_%H%M%S")
        + f"_{args.scenario}.log"
    )
    out_path = RESULTS_DIR / fname
    out_path.write_text("\n".join(log), encoding="utf-8")

    wire.close()

    # Echo summary to stdout
    print(f"\n--- result ({elapsed:.1f}s, {result['turns']} turns) ---")
    if result["summary"]:
        print(result["summary"])
    else:
        print("(no summary — see log)")
    print(f"\nfull log: {out_path}")

    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""
Simple script to send messages to the spoof Twitch IRC server via HTTP API.
Usage: python send_message.py <channel> <username> <message>
"""

import sys
import json
import urllib.request
import urllib.parse
from typing import Dict, Any

API_BASE_URL = "http://127.0.0.1:8080"
SEND_MESSAGE_ENDPOINT = f"{API_BASE_URL}/send_message"


def send_message(channel: str, username: str, message: str) -> bool:
    """Send a message to the spoof IRC server via HTTP API."""

    # Prepare the JSON payload
    payload = {"channel": channel, "username": username, "message": message}

    try:
        # Convert payload to JSON
        json_data = json.dumps(payload).encode("utf-8")

        # Create the HTTP request
        req = urllib.request.Request(
            SEND_MESSAGE_ENDPOINT,
            data=json_data,
            headers={
                "Content-Type": "application/json",
                "Content-Length": str(len(json_data)),
            },
            method="POST",
        )

        # Send the request
        with urllib.request.urlopen(req) as response:
            response_data = response.read().decode("utf-8")
            response_json = json.loads(response_data)

            if response_json.get("success"):
                print(f"✅ {response_json.get('message', 'Message sent successfully')}")
                return True
            else:
                print(f"❌ Failed: {response_json.get('message', 'Unknown error')}")
                return False

    except urllib.error.HTTPError as e:
        print(f"❌ HTTP Error {e.code}: {e.reason}")
        return False
    except urllib.error.URLError as e:
        print(f"❌ Connection Error: {e.reason}")
        print("Make sure the spoof IRC server is running on localhost:8080")
        return False
    except json.JSONDecodeError as e:
        print(f"❌ JSON Error: {e}")
        return False
    except Exception as e:
        print(f"❌ Unexpected Error: {e}")
        return False


def main():
    """Main function to handle command line arguments."""
    if len(sys.argv) != 4:
        print("Usage: python send_message.py <channel> <username> <message>")
        print()
        print("Example:")
        print("  python send_message.py mychannel viewer123 'Testing the API'")
        print()
        print("Note: Make sure the spoof IRC server is running first.")
        sys.exit(1)

    channel = sys.argv[1]
    username = sys.argv[2]
    message = sys.argv[3]

    print(f"Sending message to #{channel} from {username}: {message}")

    success = send_message(channel, username, message)
    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()


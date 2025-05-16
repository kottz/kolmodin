document.addEventListener('DOMContentLoaded', () => {
    // --- DOM Elements ---
    const createLobbyBtn = document.getElementById('createLobbyBtn');
    const gameTypeSelect = document.getElementById('gameTypeSelect');
    const twitchChannelInput = document.getElementById('twitchChannelInput');
    const lobbyIdDisplay = document.getElementById('lobbyIdDisplay');
    const gameTypeCreatedDisplay = document.getElementById('gameTypeCreatedDisplay');
    const twitchChannelSubscribedDisplay = document.getElementById('twitchChannelSubscribedDisplay');
    const lobbyIdInput = document.getElementById('lobbyIdInput');
    const connectBtn = document.getElementById('connectBtn');
    const disconnectBtn = document.getElementById('disconnectBtn');
    const connectionStatus = document.getElementById('connectionStatus');
    const twitchIrcStatusDisplay = document.getElementById('twitchIrcStatusDisplay');
    const logOutput = document.getElementById('logOutput');
    const messageInput = document.getElementById('messageInput');
    const sendMessageSection = document.getElementById('sendMessageSection');

    // --- Dynamic Buttons ---
    const sendToSelfBtn = document.createElement('button');
    const broadcastAllBtn = document.createElement('button');
    const echoBtn = document.createElement('button'); // New button for Echo command

    sendToSelfBtn.id = 'sendToSelfBtn';
    sendToSelfBtn.textContent = 'Send to Self (Private)';
    sendToSelfBtn.style.marginRight = '10px';
    sendToSelfBtn.style.marginTop = '5px';

    broadcastAllBtn.id = 'broadcastAllBtn';
    broadcastAllBtn.textContent = 'Broadcast to All';
    broadcastAllBtn.style.marginRight = '10px';
    broadcastAllBtn.style.marginTop = '5px';

    echoBtn.id = 'echoBtn';
    echoBtn.textContent = 'Echo Message';
    echoBtn.style.marginTop = '5px';


    if (sendMessageSection) {
        sendMessageSection.appendChild(echoBtn);
        sendMessageSection.appendChild(sendToSelfBtn);
        sendMessageSection.appendChild(broadcastAllBtn);
    } else {
        console.error("Could not find the 'sendMessageSection' to add new buttons.");
    }


    let webSocket = null;
    // --- Configuration ---
    const SERVER_PORT = 3000; // Match your Rust server's port
    const SERVER_HTTP_URL = `http://localhost:${SERVER_PORT}`;
    const SERVER_WS_URL = `ws://localhost:${SERVER_PORT}`;

    function logMessage(message, type = 'system') {
        const p = document.createElement('p');
        p.textContent = `[${new Date().toLocaleTimeString()}] ${message}`;
        p.classList.add(`log-${type}`);
        logOutput.appendChild(p);
        logOutput.scrollTop = logOutput.scrollHeight;
    }

    function updateUIConnected(lobbyId) {
        connectionStatus.textContent = `Connected to Lobby: ${lobbyId}`;
        connectionStatus.style.color = 'green';
        connectBtn.disabled = true;
        lobbyIdInput.disabled = true;
        disconnectBtn.disabled = false;
        messageInput.disabled = false;
        sendToSelfBtn.disabled = false;
        broadcastAllBtn.disabled = false;
        echoBtn.disabled = false;
    }

    function updateUIDisconnected() {
        connectionStatus.textContent = 'Not Connected';
        connectionStatus.style.color = 'red';
        twitchIrcStatusDisplay.textContent = 'N/A'; // Reset Twitch status
        lobbyIdInput.disabled = false;
        connectBtn.disabled = lobbyIdInput.value.trim() === '';
        disconnectBtn.disabled = true;
        messageInput.disabled = true;
        messageInput.value = '';
        sendToSelfBtn.disabled = true;
        broadcastAllBtn.disabled = true;
        echoBtn.disabled = true;
        webSocket = null;
    }

    lobbyIdInput.addEventListener('input', () => {
        connectBtn.disabled = !(lobbyIdInput.value.trim() !== '' && !webSocket);
    });

    createLobbyBtn.addEventListener('click', async () => {
        logMessage('Attempting to create lobby...');
        const selectedGameType = gameTypeSelect.value;
        const requestedTwitchChannel = twitchChannelInput.value.trim();

        const payload = {
            game_type: selectedGameType === 'default' ? null : selectedGameType,
            twitch_channel: requestedTwitchChannel ? requestedTwitchChannel : null
        };

        try {
            const response = await fetch(`${SERVER_HTTP_URL}/api/create-lobby`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(payload),
            });

            if (!response.ok) {
                let errorText = `HTTP error! status: ${response.status}`;
                try {
                    const errorData = await response.json(); // Server sends JSON for errors too now if using the Json Error type
                    errorText = (errorData && errorData.message) ? errorData.message : `Server error: ${response.statusText}`;
                } catch (e) {
                    // If server sends (StatusCode, String) for error
                    try {
                        errorText = await response.text();
                    } catch (e2) { /* ignore */ }
                }
                throw new Error(errorText);
            }

            const data = await response.json();
            const newLobbyId = data.lobby_id;
            const gameCreated = data.game_type_created;
            const twitchSubscribed = data.twitch_channel_subscribed || '-';

            lobbyIdDisplay.textContent = newLobbyId;
            gameTypeCreatedDisplay.textContent = gameCreated;
            twitchChannelSubscribedDisplay.textContent = twitchSubscribed;
            lobbyIdInput.value = newLobbyId;
            logMessage(`Lobby created: ${newLobbyId} (Game: ${gameCreated}, Twitch: ${twitchSubscribed})`, 'system');
            if (!webSocket) connectBtn.disabled = false;
        } catch (error) {
            logMessage(`Error creating lobby: ${error.message}`, 'error');
            console.error('Error creating lobby:', error);
        }
    });

    connectBtn.addEventListener('click', () => {
        const lobbyId = lobbyIdInput.value.trim();
        if (!lobbyId) {
            logMessage('Please enter a Lobby ID to connect.', 'error');
            return;
        }
        if (webSocket) {
            logMessage('Already connected or attempting to connect.', 'system');
            return;
        }

        logMessage(`Attempting to connect to lobby: ${lobbyId}...`);
        const wsUrl = `${SERVER_WS_URL}/ws/${lobbyId}`;
        try {
            webSocket = new WebSocket(wsUrl);
        } catch (e) {
            logMessage(`Error creating WebSocket: ${e.message}`, 'error');
            console.error('Error creating WebSocket:', e);
            updateUIDisconnected();
            return;
        }

        webSocket.onopen = () => {
            logMessage(`Successfully connected to WebSocket for lobby: ${lobbyId}`, 'system');
            updateUIConnected(lobbyId);
        };

        webSocket.onmessage = (event) => {
            const rawMessageData = event.data;
            logMessage(`Raw Received: ${rawMessageData}`, 'received-raw'); // Log raw data

            try {
                const parsedMessage = JSON.parse(rawMessageData);
                // `parsedMessage` should now be an object like:
                // { "event": "EventType", "data": { ...payload... } }

                logMessage(`Parsed Received (${parsedMessage.event}): ${JSON.stringify(parsedMessage.data)}`, 'received');

                switch (parsedMessage.event) {
                    case 'EchoResponse':
                        logMessage(`Server Echo: Original='${parsedMessage.data.original}', Processed='${parsedMessage.data.processed}'`, 'game');
                        break;
                    case 'PrivateMessage':
                        logMessage(`Server Private: ${parsedMessage.data.content}`, 'game');
                        break;
                    case 'BroadcastMessage':
                        logMessage(`Server Broadcast: ${parsedMessage.data.content}`, 'game');
                        break;
                    case 'TwitchMessageRelay':
                        logMessage(`[Twitch Chat #${parsedMessage.data.channel}] ${parsedMessage.data.sender}: ${parsedMessage.data.text}`, 'twitch-chat');
                        break;
                    case 'GameUpdate': // For generic game state
                        logMessage(`Game Update: ${JSON.stringify(parsedMessage.data.update_data)}`, 'game-update');
                        // You would handle specific game updates based on the content of update_data
                        break;
                    case 'Error':
                        logMessage(`Server Error: ${parsedMessage.data.message}`, 'error');
                        break;
                    // --- Handle Twitch Status (This part is more complex as Twitch status is not directly part of ServerToClientMessage yet)
                    // For now, we'll assume Twitch status updates might come through a generic `GameUpdate` or a custom event if you add one.
                    // Or, if your LobbyActor sends a special `ServerToClientMessage::GameUpdate` with Twitch status info:
                    // Example: server sends `{"event":"GameUpdate","data":{"update_data": {"type": "twitch_status", "channel_name": "...", "status_type": "..."}}}`
                    default:
                        // This case handles messages that are valid JSON but not one of the recognized events.
                        // It could also be where your custom Twitch status messages (if not fitting above) might land.
                        if (parsedMessage.type === 'twitch_status_update' && parsedMessage.data) { // Keeping this for potential direct Twitch status handling
                            const status = parsedMessage.data;
                            let statusText = `Channel: ${status.channel_name || twitchChannelSubscribedDisplay.textContent}, Status: ${status.status_type}`;
                            if (status.details) statusText += ` (${status.details})`;
                            twitchIrcStatusDisplay.textContent = statusText;
                            logMessage(`Twitch IRC Status Update: ${statusText}`, 'system-twitch');
                        } else {
                            logMessage(`Received unhandled structured message: ${rawMessageData}`, 'system');
                        }
                        break;
                }
            } catch (e) {
                // If it's not JSON, it might be an older message format or an unexpected string.
                logMessage(`Received non-JSON message or parse error: ${rawMessageData}`, 'system');
                console.warn("Failed to parse message as JSON or unknown structure:", e, rawMessageData);
            }
        };

        webSocket.onerror = (errorEvent) => {
            logMessage(`WebSocket Error. Check browser console for details.`, 'error');
            console.error('WebSocket Error Event:', errorEvent);
        };

        webSocket.onclose = (event) => {
            let reason = '';
            if (event.code) reason += `Code: ${event.code}`;
            if (event.reason) reason += ` Reason: ${event.reason}`;
            if (!reason && !event.wasClean) reason = 'Connection failed or closed unexpectedly.';
            else if (!reason && event.wasClean) reason = 'Connection closed cleanly.';
            logMessage(`WebSocket disconnected. ${reason}`, event.wasClean ? 'system' : 'error');
            updateUIDisconnected();
        };
    });

    disconnectBtn.addEventListener('click', () => {
        if (webSocket) {
            logMessage('Disconnecting...', 'system');
            webSocket.close();
        }
    });

    /**
     * Sends a structured command to the WebSocket server.
     * @param {string} commandName - The 'command' field for ClientToServerMessage (e.g., "Echo", "SendToSelf").
     * @param {object} payloadData - The data for the 'payload' field.
     */
    function sendStructuredCommand(commandName, payloadData) {
        if (webSocket && webSocket.readyState === WebSocket.OPEN) {
            const messageToSend = {
                command: commandName,
                payload: payloadData
            };
            const jsonMessage = JSON.stringify(messageToSend);
            logMessage(`Sending Command '${commandName}': ${jsonMessage}`, 'sent');
            webSocket.send(jsonMessage);
        } else {
            logMessage('WebSocket is not connected. Cannot send command.', 'error');
        }
    }

    echoBtn.addEventListener('click', () => {
        const userMessage = messageInput.value.trim();
        if (userMessage === "") {
            logMessage("Please type a message for Echo.", "error");
            messageInput.focus();
            return;
        }
        sendStructuredCommand("Echo", { message: userMessage });
    });

    sendToSelfBtn.addEventListener('click', () => {
        const userMessage = messageInput.value.trim();
        // if (userMessage === "") { // Optional: decide if empty messages are allowed
        //     logMessage("Please type a message for SendToSelf.", "error");
        //     messageInput.focus();
        //     return;
        // }
        sendStructuredCommand("SendToSelf", { message: userMessage });
    });

    broadcastAllBtn.addEventListener('click', () => {
        const userMessage = messageInput.value.trim();
        // if (userMessage === "") { // Optional
        //     logMessage("Please type a message for BroadcastAll.", "error");
        //     messageInput.focus();
        //     return;
        // }
        sendStructuredCommand("BroadcastAll", { message: userMessage });
    });

    updateUIDisconnected(); // Initial UI state
});

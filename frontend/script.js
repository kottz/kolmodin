document.addEventListener('DOMContentLoaded', () => {
    // --- DOM Elements ---
    const createLobbyBtn = document.getElementById('createLobbyBtn');
    const gameTypeSelect = document.getElementById('gameTypeSelect');
    const twitchChannelInput = document.getElementById('twitchChannelInput'); // New
    const lobbyIdDisplay = document.getElementById('lobbyIdDisplay');
    const gameTypeCreatedDisplay = document.getElementById('gameTypeCreatedDisplay');
    const twitchChannelSubscribedDisplay = document.getElementById('twitchChannelSubscribedDisplay'); // New
    const lobbyIdInput = document.getElementById('lobbyIdInput');
    const connectBtn = document.getElementById('connectBtn');
    const disconnectBtn = document.getElementById('disconnectBtn');
    const connectionStatus = document.getElementById('connectionStatus');
    const twitchIrcStatusDisplay = document.getElementById('twitchIrcStatusDisplay'); // New
    const logOutput = document.getElementById('logOutput');
    const messageInput = document.getElementById('messageInput');
    const sendMessageSection = document.getElementById('sendMessageSection');

    // --- Dynamic Buttons (No change here) ---
    const sendToSelfBtn = document.createElement('button'); /* ... */
    const broadcastAllBtn = document.createElement('button'); /* ... */
    const broadcastOthersBtn = document.createElement('button'); /* ... */
    // Code to append buttons remains the same
    sendToSelfBtn.id = 'sendToSelfBtn';
    sendToSelfBtn.textContent = 'Send to Self';
    sendToSelfBtn.style.marginRight = '10px';
    sendToSelfBtn.style.marginTop = '5px';
    broadcastAllBtn.id = 'broadcastAllBtn';
    broadcastAllBtn.textContent = 'Broadcast to All';
    broadcastAllBtn.style.marginRight = '10px';
    broadcastAllBtn.style.marginTop = '5px';
    broadcastOthersBtn.id = 'broadcastOthersBtn';
    broadcastOthersBtn.textContent = 'Broadcast to Others';
    broadcastOthersBtn.style.marginTop = '5px';
    if (sendMessageSection) {
        sendMessageSection.appendChild(sendToSelfBtn);
        sendMessageSection.appendChild(broadcastAllBtn);
        sendMessageSection.appendChild(broadcastOthersBtn);
    } else {
        console.error("Could not find the 'sendMessageSection' to add new buttons.");
    }


    let webSocket = null;
    // --- Configuration ---
    // Update these if your server runs on a different port or host
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
        broadcastOthersBtn.disabled = false;
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
        broadcastOthersBtn.disabled = true;
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
            // Send twitch_channel only if it's not empty
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
                    const errorData = await response.json();
                    errorText = (errorData && errorData.length > 0) ? errorData : `Server error: ${response.statusText}`; // Axum returns plain string for (StatusCode, String)
                } catch (e) { /* Ignore if error response isn't JSON */ }
                throw new Error(errorText);
            }

            const data = await response.json(); // Expecting { lobby_id, game_type_created, twitch_channel_subscribed }
            const newLobbyId = data.lobby_id;
            const gameCreated = data.game_type_created;
            const twitchSubscribed = data.twitch_channel_subscribed || '-'; // Display '-' if null/undefined

            lobbyIdDisplay.textContent = newLobbyId;
            gameTypeCreatedDisplay.textContent = gameCreated;
            twitchChannelSubscribedDisplay.textContent = twitchSubscribed; // Display Twitch channel
            lobbyIdInput.value = newLobbyId; // Pre-fill connect input
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
            // After connecting, if the lobby is subscribed to a Twitch channel,
            // the server might send initial status or messages.
            // We might also want a way for the client to explicitly request Twitch status.
        };

        webSocket.onmessage = (event) => {
            const messageData = event.data;
            logMessage(`Received: ${messageData}`, 'received');

            // Attempt to parse as JSON for potential structured messages (like Twitch status)
            try {
                const parsed = JSON.parse(messageData);
                if (parsed.type === 'twitch_status_update' && parsed.data) {
                    const status = parsed.data;
                    let statusText = `Channel: ${status.channel_name || twitchChannelSubscribedDisplay.textContent}, Status: ${status.status_type}`;
                    if (status.details) statusText += ` (${status.details})`;
                    twitchIrcStatusDisplay.textContent = statusText;
                    logMessage(`Twitch IRC Status Update: ${statusText}`, 'system-twitch');
                }
                // Add more structured message handlers here if needed
            } catch (e) {
                // If not JSON, or not a recognized structured message, log as plain text (already done)
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

    function sendCommandWithMessage(commandType) {
        if (webSocket && webSocket.readyState === WebSocket.OPEN) {
            const userMessage = messageInput.value.trim();
            // Example: some commands might not need a payload.
            // For this demo, all example commands take the messageInput content.
            // if (commandType !== "some_command_without_payload" && userMessage === "") {
            //     logMessage("Please type a message before sending.", "error");
            //     messageInput.focus();
            //     return;
            // }
            const fullMessage = `${commandType} ${userMessage}`; // Format: "command payload"
            logMessage(`Sending: ${fullMessage}`, 'sent');
            webSocket.send(fullMessage);
            // messageInput.value = ''; // Optionally clear input after sending
        } else {
            logMessage('WebSocket is not connected. Cannot send command.', 'error');
        }
    }

    sendToSelfBtn.addEventListener('click', () => sendCommandWithMessage("send_to_self"));
    broadcastAllBtn.addEventListener('click', () => sendCommandWithMessage("broadcast_all"));
    broadcastOthersBtn.addEventListener('click', () => sendCommandWithMessage("broadcast_except_self"));

    updateUIDisconnected(); // Initial UI state
});

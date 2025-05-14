document.addEventListener('DOMContentLoaded', () => {
    // --- DOM Elements ---
    const createLobbyBtn = document.getElementById('createLobbyBtn');
    const gameTypeSelect = document.getElementById('gameTypeSelect'); // Get game type selector
    const lobbyIdDisplay = document.getElementById('lobbyIdDisplay');
    const gameTypeCreatedDisplay = document.getElementById('gameTypeCreatedDisplay'); // For showing created game type
    const lobbyIdInput = document.getElementById('lobbyIdInput');
    const connectBtn = document.getElementById('connectBtn');
    const disconnectBtn = document.getElementById('disconnectBtn');
    const connectionStatus = document.getElementById('connectionStatus');
    const logOutput = document.getElementById('logOutput');
    const messageInput = document.getElementById('messageInput');

    const sendMessageSection = document.getElementById('sendMessageSection');

    const sendToSelfBtn = document.createElement('button');
    sendToSelfBtn.id = 'sendToSelfBtn';
    sendToSelfBtn.textContent = 'Send to Self';
    sendToSelfBtn.style.marginRight = '10px';
    sendToSelfBtn.style.marginTop = '5px';

    const broadcastAllBtn = document.createElement('button');
    broadcastAllBtn.id = 'broadcastAllBtn';
    broadcastAllBtn.textContent = 'Broadcast to All';
    broadcastAllBtn.style.marginRight = '10px';
    broadcastAllBtn.style.marginTop = '5px';

    const broadcastOthersBtn = document.createElement('button');
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
    const SERVER_HTTP_URL = 'http://localhost:3000';
    const SERVER_WS_URL = 'ws://localhost:3000';

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
        if (lobbyIdInput.value.trim() !== '' && !webSocket) {
            connectBtn.disabled = false;
        } else {
            connectBtn.disabled = true;
        }
    });

    createLobbyBtn.addEventListener('click', async () => {
        logMessage('Attempting to create lobby...');
        const selectedGameType = gameTypeSelect.value;

        // Prepare the payload for the server
        const payload = {
            // Send null if 'default' is selected, so server uses its default.
            // Otherwise, send the selected game type string.
            game_type: selectedGameType === 'default' ? null : selectedGameType
        };

        try {
            const response = await fetch(`${SERVER_HTTP_URL}/api/create-lobby`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json', // Crucial header
                },
                body: JSON.stringify(payload), // Send the JSON payload
            });

            if (!response.ok) {
                // Attempt to get more specific error message from server if available
                let errorText = `HTTP error! status: ${response.status}`;
                try {
                    const errorData = await response.json(); // Axum often sends JSON error responses
                    if (errorData && errorData.message) { // Check for a common error message pattern
                        errorText += ` - ${errorData.message}`;
                    } else {
                        const text = await response.text(); // Fallback to plain text
                        errorText += ` - ${text}`;
                    }
                } catch (e) {
                    // If parsing error response fails, just use the status
                }
                throw new Error(errorText);
            }

            const data = await response.json(); // Expecting LobbyDetails { lobby_id, game_type_created }
            const newLobbyId = data.lobby_id;
            const gameCreated = data.game_type_created;

            lobbyIdDisplay.textContent = newLobbyId;
            gameTypeCreatedDisplay.textContent = gameCreated; // Display the game type created by server
            lobbyIdInput.value = newLobbyId;
            logMessage(`Lobby created: ${newLobbyId} (Game: ${gameCreated})`, 'system');
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
            logMessage(`Received: ${event.data}`, 'received');
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
            if (commandType !== "some_command_without_payload" && userMessage === "") { // Example if some commands don't need payload
                logMessage("Please type a message before sending.", "error");
                messageInput.focus();
                return;
            }
            const fullMessage = `${commandType} ${userMessage}`;
            logMessage(`Sending: ${fullMessage}`, 'sent');
            webSocket.send(fullMessage);
        } else {
            logMessage('WebSocket is not connected. Cannot send command.', 'error');
        }
    }

    sendToSelfBtn.addEventListener('click', () => {
        sendCommandWithMessage("send_to_self");
    });

    broadcastAllBtn.addEventListener('click', () => {
        sendCommandWithMessage("broadcast_all");
    });

    broadcastOthersBtn.addEventListener('click', () => {
        sendCommandWithMessage("broadcast_except_self");
    });

    updateUIDisconnected();
});

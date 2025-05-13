document.addEventListener('DOMContentLoaded', () => {
    // --- DOM Elements ---
    const createLobbyBtn = document.getElementById('createLobbyBtn');
    const lobbyIdDisplay = document.getElementById('lobbyIdDisplay');
    const lobbyIdInput = document.getElementById('lobbyIdInput');
    const connectBtn = document.getElementById('connectBtn');
    const disconnectBtn = document.getElementById('disconnectBtn');
    const connectionStatus = document.getElementById('connectionStatus');
    const logOutput = document.getElementById('logOutput');
    const messageInput = document.getElementById('messageInput'); // Get the message input

    // --- New Buttons for Sending Commands ---
    const sendMessageSection = document.getElementById('sendMessageSection');

    const sendToSelfBtn = document.createElement('button');
    sendToSelfBtn.id = 'sendToSelfBtn';
    sendToSelfBtn.textContent = 'Send to Self';
    sendToSelfBtn.style.marginRight = '10px';
    sendToSelfBtn.style.marginTop = '5px'; // Add some top margin

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
        // Append new buttons after the messageInput
        sendMessageSection.appendChild(sendToSelfBtn);
        sendMessageSection.appendChild(broadcastAllBtn);
        sendMessageSection.appendChild(broadcastOthersBtn);
    } else {
        console.error("Could not find the 'sendMessageSection' to add new buttons.");
    }

    // --- WebSocket and Server Configuration ---
    let webSocket = null;
    const SERVER_HTTP_URL = 'http://localhost:3000';
    const SERVER_WS_URL = 'ws://localhost:3000';

    // --- Logging Function ---
    function logMessage(message, type = 'system') {
        const p = document.createElement('p');
        p.textContent = `[${new Date().toLocaleTimeString()}] ${message}`;
        p.classList.add(`log-${type}`);
        logOutput.appendChild(p);
        logOutput.scrollTop = logOutput.scrollHeight;
    }

    // --- UI Update Functions ---
    function updateUIConnected(lobbyId) {
        connectionStatus.textContent = `Connected to Lobby: ${lobbyId}`;
        connectionStatus.style.color = 'green';
        connectBtn.disabled = true;
        lobbyIdInput.disabled = true;
        disconnectBtn.disabled = false;
        messageInput.disabled = false; // Enable message input

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
        messageInput.disabled = true; // Disable message input
        messageInput.value = ''; // Clear message input

        sendToSelfBtn.disabled = true;
        broadcastAllBtn.disabled = true;
        broadcastOthersBtn.disabled = true;

        webSocket = null;
    }

    // --- Event Listeners for UI Elements ---
    lobbyIdInput.addEventListener('input', () => {
        if (lobbyIdInput.value.trim() !== '' && !webSocket) {
            connectBtn.disabled = false;
        } else {
            connectBtn.disabled = true;
        }
    });

    createLobbyBtn.addEventListener('click', async () => {
        logMessage('Attempting to create lobby...');
        try {
            const response = await fetch(`${SERVER_HTTP_URL}/api/create-lobby`, {
                method: 'POST',
            });
            if (!response.ok) {
                const errorText = await response.text();
                throw new Error(`HTTP error! status: ${response.status} - ${errorText}`);
            }
            const data = await response.json();
            const newLobbyId = data.lobby_id;
            lobbyIdDisplay.textContent = newLobbyId;
            lobbyIdInput.value = newLobbyId;
            logMessage(`Lobby created: ${newLobbyId}`, 'system');
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

    // --- Function to Send Commands with Message Payload via WebSocket ---
    function sendCommandWithMessage(commandType) {
        if (webSocket && webSocket.readyState === WebSocket.OPEN) {
            const userMessage = messageInput.value.trim();
            if (userMessage === "") {
                logMessage("Please type a message before sending.", "error");
                messageInput.focus();
                return;
            }
            // Format: COMMAND_TYPE<space>Actual message
            const fullMessage = `${commandType} ${userMessage}`;
            logMessage(`Sending: ${fullMessage}`, 'sent');
            webSocket.send(fullMessage);
            // Optionally clear the input after sending
            // messageInput.value = '';
        } else {
            logMessage('WebSocket is not connected. Cannot send command.', 'error');
        }
    }

    // --- Event Listeners for New Command Buttons ---
    sendToSelfBtn.addEventListener('click', () => {
        sendCommandWithMessage("send_to_self");
    });

    broadcastAllBtn.addEventListener('click', () => {
        sendCommandWithMessage("broadcast_all");
    });

    broadcastOthersBtn.addEventListener('click', () => {
        sendCommandWithMessage("broadcast_except_self");
    });

    // --- Initial UI State ---
    updateUIDisconnected();
});

document.addEventListener('DOMContentLoaded', () => {
    // --- DOM Elements ---
    const createLobbyBtn = document.getElementById('createLobbyBtn');
    const lobbyIdDisplay = document.getElementById('lobbyIdDisplay');
    const lobbyIdInput = document.getElementById('lobbyIdInput');
    const connectBtn = document.getElementById('connectBtn');
    const disconnectBtn = document.getElementById('disconnectBtn');
    const connectionStatus = document.getElementById('connectionStatus');
    const logOutput = document.getElementById('logOutput');

    // --- New Buttons for Sending Commands ---
    // Assuming the original sendBtn and messageInput are in a div with class "section"
    // and we want to replace them.
    // If your HTML structure is different, you might need to adjust how these are added.
    const oldSendBtn = document.getElementById('sendBtn');
    const oldMessageInput = document.getElementById('messageInput');
    const sendSection = oldSendBtn ? oldSendBtn.parentElement : document.querySelector('.section:nth-of-type(3)'); // Fallback selector

    const sendToSelfBtn = document.createElement('button');
    sendToSelfBtn.id = 'sendToSelfBtn';
    sendToSelfBtn.textContent = 'Send "Hello World" to Self';
    sendToSelfBtn.style.marginRight = '10px'; // Add some spacing

    const broadcastAllBtn = document.createElement('button');
    broadcastAllBtn.id = 'broadcastAllBtn';
    broadcastAllBtn.textContent = 'Broadcast "Hello World" to All';
    broadcastAllBtn.style.marginRight = '10px'; // Add some spacing

    const broadcastOthersBtn = document.createElement('button');
    broadcastOthersBtn.id = 'broadcastOthersBtn';
    broadcastOthersBtn.textContent = 'Broadcast "Hello World" to Others';

    if (sendSection) {
        // Clear out old send elements if they exist
        if (oldSendBtn) sendSection.removeChild(oldSendBtn);
        if (oldMessageInput) sendSection.removeChild(oldMessageInput);

        // Add new buttons
        sendSection.appendChild(sendToSelfBtn);
        sendSection.appendChild(broadcastAllBtn);
        sendSection.appendChild(broadcastOthersBtn);
    } else {
        console.error("Could not find the 'Send Message' section to add new buttons.");
    }


    // --- WebSocket and Server Configuration ---
    let webSocket = null;
    const SERVER_HTTP_URL = 'http://localhost:3000';
    const SERVER_WS_URL = 'ws://localhost:3000';

    // --- Logging Function ---
    function logMessage(message, type = 'system') {
        const p = document.createElement('p');
        p.textContent = `[${new Date().toLocaleTimeString()}] ${message}`;
        p.classList.add(`log-${type}`); // For CSS styling (log-sent, log-received, etc.)
        logOutput.appendChild(p);
        logOutput.scrollTop = logOutput.scrollHeight; // Auto-scroll to bottom
    }

    // --- UI Update Functions ---
    function updateUIConnected(lobbyId) {
        connectionStatus.textContent = `Connected to Lobby: ${lobbyId}`;
        connectionStatus.style.color = 'green';
        connectBtn.disabled = true;
        lobbyIdInput.disabled = true;
        disconnectBtn.disabled = false;

        // Enable new command buttons
        sendToSelfBtn.disabled = false;
        broadcastAllBtn.disabled = false;
        broadcastOthersBtn.disabled = false;
    }

    function updateUIDisconnected() {
        connectionStatus.textContent = 'Not Connected';
        connectionStatus.style.color = 'red';
        lobbyIdInput.disabled = false;
        // Enable connect button only if there's a lobby ID in the input
        connectBtn.disabled = lobbyIdInput.value.trim() === '';
        disconnectBtn.disabled = true;

        // Disable new command buttons
        sendToSelfBtn.disabled = true;
        broadcastAllBtn.disabled = true;
        broadcastOthersBtn.disabled = true;

        webSocket = null; // Clear WebSocket object
    }

    // --- Event Listeners for UI Elements ---
    lobbyIdInput.addEventListener('input', () => {
        // Enable connect button if there's text and not already connected
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
            lobbyIdInput.value = newLobbyId; // Populate input for easy connect
            logMessage(`Lobby created: ${newLobbyId}`, 'system');
            if (!webSocket) connectBtn.disabled = false; // Enable connect if not already connected
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
            // The error event itself doesn't have much detail for failed connections.
            // The browser console usually has more info (e.g., "WebSocket connection to '...' failed").
            logMessage(`WebSocket Error. Check browser console for details.`, 'error');
            console.error('WebSocket Error Event:', errorEvent);
            // onclose will usually be called after an error.
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
            // The onclose event handler will call updateUIDisconnected()
        }
    });

    // --- Function to Send Commands via WebSocket ---
    function sendMessageCommand(command) {
        if (webSocket && webSocket.readyState === WebSocket.OPEN) {
            logMessage(`Sending command: ${command}`, 'sent');
            webSocket.send(command);
        } else {
            logMessage('WebSocket is not connected. Cannot send command.', 'error');
        }
    }

    // --- Event Listeners for New Command Buttons ---
    sendToSelfBtn.addEventListener('click', () => {
        sendMessageCommand("send_to_self");
    });

    broadcastAllBtn.addEventListener('click', () => {
        sendMessageCommand("broadcast_all");
    });

    broadcastOthersBtn.addEventListener('click', () => {
        sendMessageCommand("broadcast_except_self");
    });

    // --- Initial UI State ---
    updateUIDisconnected();
});

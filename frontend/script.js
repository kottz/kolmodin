// script.js
window.gameHandlers = window.gameHandlers || {}; // Ensure it exists

document.addEventListener('DOMContentLoaded', () => {
    // --- Global Game Handler Registry ---

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

    // Game Specific UI
    const currentGameTypeDisplay = document.getElementById('currentGameTypeDisplay');
    const gameSpecificControls = document.getElementById('gameSpecificControls');
    const latestGameEventOutput = document.getElementById('latestGameEventOutput');

    // Global Commands UI
    const globalMessageInput = document.getElementById('globalMessageInput');
    const globalEchoBtn = document.getElementById('globalEchoBtn');


    let webSocket = null;
    let currentLobbyId = null;
    let activeGameType = null; // To store the game_type_id of the connected game

    // --- Configuration ---
    const SERVER_PORT = 3000;
    const SERVER_HTTP_URL = `http://localhost:${SERVER_PORT}`;
    const SERVER_WS_URL = `ws://localhost:${SERVER_PORT}`;

    function logMessage(message, type = 'system') {
        const p = document.createElement('p');
        p.textContent = `[${new Date().toLocaleTimeString()}] ${message}`;
        p.className = `log-${type}`; // Use className for single class
        logOutput.appendChild(p);
        logOutput.scrollTop = logOutput.scrollHeight; // Auto-scroll
    }

    function updateUIConnected(lobbyId, gameType) {
        connectionStatus.textContent = `Connected to Lobby: ${lobbyId}`;
        connectionStatus.style.color = 'green';
        connectBtn.disabled = true;
        lobbyIdInput.disabled = true;
        disconnectBtn.disabled = false;

        activeGameType = gameType;
        currentGameTypeDisplay.textContent = gameType || "Unknown Game";
        loadGameUI(gameType);
        globalMessageInput.disabled = false;
        globalEchoBtn.disabled = false;
    }

    function updateUIDisconnected() {
        connectionStatus.textContent = 'Not Connected';
        connectionStatus.style.color = 'red';
        twitchIrcStatusDisplay.textContent = 'N/A';
        lobbyIdInput.disabled = false;
        connectBtn.disabled = lobbyIdInput.value.trim() === '';
        disconnectBtn.disabled = true;

        activeGameType = null;
        currentGameTypeDisplay.textContent = "No Game Active";
        gameSpecificControls.innerHTML = '<p>Connect to a lobby with an active game to see controls.</p>';
        latestGameEventOutput.textContent = 'No game-specific event received yet.';
        globalMessageInput.disabled = true;
        globalMessageInput.value = '';
        globalEchoBtn.disabled = true;

        currentLobbyId = null;
        if (webSocket) {
            webSocket.onopen = null;
            webSocket.onmessage = null;
            webSocket.onerror = null;
            webSocket.onclose = null;
            // webSocket.close(); // Server should handle forceful close if needed on its end
        }
        webSocket = null;
    }

    function loadGameUI(gameTypeId) {
        gameSpecificControls.innerHTML = ''; // Clear previous
        if (gameTypeId && window.gameHandlers[gameTypeId] && typeof window.gameHandlers[gameTypeId].initUI === 'function') {
            window.gameHandlers[gameTypeId].initUI(gameSpecificControls, sendGameSpecificCommandToServer);
        } else {
            gameSpecificControls.innerHTML = `<p>No UI handler registered for game type: ${gameTypeId || 'Unknown'}. Generic commands may still work.</p>`;
        }
    }

    lobbyIdInput.addEventListener('input', () => {
        connectBtn.disabled = !(lobbyIdInput.value.trim() !== '' && !webSocket);
    });

    createLobbyBtn.addEventListener('click', async () => {
        logMessage('Attempting to create lobby...');
        const selectedGameType = gameTypeSelect.value; // This is now directly the game_type_id
        const requestedTwitchChannel = twitchChannelInput.value.trim();

        const payload = {
            game_type: selectedGameType, // Send the selected game_type_id
            twitch_channel: requestedTwitchChannel || null
        };

        try {
            const response = await fetch(`${SERVER_HTTP_URL}/api/create-lobby`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(payload),
            });

            const responseData = await response.json();
            if (!response.ok) {
                throw new Error(responseData.message || `HTTP error! status: ${response.status}`);
            }

            currentLobbyId = responseData.lobby_id;
            const gameCreated = responseData.game_type_created; // This should be the actual game_type_id
            const twitchSubscribed = responseData.twitch_channel_subscribed || '-';

            lobbyIdDisplay.textContent = currentLobbyId;
            gameTypeCreatedDisplay.textContent = gameCreated;
            twitchChannelSubscribedDisplay.textContent = twitchSubscribed;
            lobbyIdInput.value = currentLobbyId;
            logMessage(`Lobby created: ${currentLobbyId} (Game: ${gameCreated}, Twitch: ${twitchSubscribed})`, 'system');
            if (!webSocket) connectBtn.disabled = false;

            // Automatically connect if a lobby is successfully created and we're not already connected.
            // if (!webSocket && currentLobbyId) {
            //     connectBtn.click();
            // }
        } catch (error) {
            logMessage(`Error creating lobby: ${error.message}`, 'error');
            console.error('Error creating lobby:', error);
        }
    });

    connectBtn.addEventListener('click', () => {
        const lobbyIdToConnect = lobbyIdInput.value.trim();
        if (!lobbyIdToConnect) {
            logMessage('Please enter a Lobby ID to connect.', 'error');
            return;
        }
        if (webSocket) {
            logMessage('Already connected or attempting to connect.', 'system');
            return;
        }

        currentLobbyId = lobbyIdToConnect; // Store the lobby ID we are connecting to
        logMessage(`Attempting to connect to lobby: ${currentLobbyId}...`);
        const wsUrl = `${SERVER_WS_URL}/ws/${currentLobbyId}`;
        try {
            webSocket = new WebSocket(wsUrl);
        } catch (e) {
            logMessage(`Error creating WebSocket: ${e.message}`, 'error');
            console.error('Error creating WebSocket:', e);
            updateUIDisconnected();
            return;
        }

        webSocket.onopen = () => {
            // We don't know the game_type yet from onopen.
            // The server should send an initial GameSpecificEvent or GlobalEvent with this info,
            // or we can infer it from the first GameSpecificEvent.
            // For now, we'll update UI slightly and wait for messages.
            logMessage(`Successfully connected to WebSocket for lobby: ${currentLobbyId}`, 'system');
            // We expect the server to send the game_type, perhaps as part of the first GameStateUpdate
            // For now, let's assume it's what was displayed in `gameTypeCreatedDisplay`
            // A more robust way is for the server to send this upon connection.
            const assumedGameType = gameTypeCreatedDisplay.textContent === '-' ? null : gameTypeCreatedDisplay.textContent;
            updateUIConnected(currentLobbyId, assumedGameType);
        };

        webSocket.onmessage = (event) => {
            const rawMessageData = event.data;
            logMessage(`Raw Received: ${rawMessageData}`, 'received-generic');

            try {
                const parsedMessage = JSON.parse(rawMessageData);
                // New structure: { "message_type": "...", "payload": { ... } }
                logMessage(`Parsed Received (${parsedMessage.message_type}): ${JSON.stringify(parsedMessage.payload)}`, 'received-generic');

                switch (parsedMessage.message_type) {
                    case 'GlobalEvent':
                        logMessage(`Global Event (${parsedMessage.payload.event_name}): ${JSON.stringify(parsedMessage.payload.data)}`, 'received-global');
                        // Handle specific global events if needed
                        if (parsedMessage.payload.event_name === "LobbyInfo" && parsedMessage.payload.data.game_type_id) {
                            if (activeGameType !== parsedMessage.payload.data.game_type_id) {
                                activeGameType = parsedMessage.payload.data.game_type_id;
                                currentGameTypeDisplay.textContent = activeGameType;
                                loadGameUI(activeGameType); // Reload UI for the correct game
                                logMessage(`Game type for lobby ${currentLobbyId} confirmed: ${activeGameType}`, 'system');
                            }
                        }
                        // Example for Twitch status if sent as a GlobalEvent
                        if (parsedMessage.payload.event_name === "TwitchStatusUpdate" && parsedMessage.payload.data) {
                            const status = parsedMessage.payload.data;
                            let statusText = `Channel: ${status.channel_name || twitchChannelSubscribedDisplay.textContent}, Status: ${status.status_type}`;
                            if (status.details) statusText += ` (${status.details})`;
                            twitchIrcStatusDisplay.textContent = statusText;
                            logMessage(`Twitch IRC Status Update: ${statusText}`, 'system');
                        }
                        break;

                    case 'GameSpecificEvent':
                        logMessage(`Game Event for ${parsedMessage.payload.game_type_id}: ${JSON.stringify(parsedMessage.payload.event_data)}`, 'received-game');
                        if (activeGameType !== parsedMessage.payload.game_type_id) {
                            // This means we might have connected without knowing the game type,
                            // or it changed (less likely for this app's design).
                            logMessage(`Received event for game ${parsedMessage.payload.game_type_id}, but current active game is ${activeGameType}. Updating active game.`, 'system');
                            activeGameType = parsedMessage.payload.game_type_id;
                            currentGameTypeDisplay.textContent = activeGameType;
                            loadGameUI(activeGameType); // Load UI for the received game type
                        }

                        // Dispatch to the specific game handler
                        if (activeGameType && window.gameHandlers[activeGameType] && typeof window.gameHandlers[activeGameType].handleGameEvent === 'function') {
                            window.gameHandlers[activeGameType].handleGameEvent(parsedMessage.payload.event_data, latestGameEventOutput);
                        } else {
                            latestGameEventOutput.textContent = `No handler for ${activeGameType} or event_data missing.\nRaw: ${JSON.stringify(parsedMessage.payload.event_data, null, 2)}`;
                        }
                        break;

                    case 'SystemError':
                        logMessage(`Server System Error: ${parsedMessage.payload.message}`, 'error');
                        break;

                    case 'TwitchMessageRelay': // This is now a top-level message_type
                        logMessage(`[Twitch Chat #${parsedMessage.payload.channel}] ${parsedMessage.payload.sender}: ${parsedMessage.payload.text}`, 'twitch-chat');
                        break;

                    default:
                        logMessage(`Received unhandled message type: ${parsedMessage.message_type}`, 'system');
                        latestGameEventOutput.textContent = JSON.stringify(parsedMessage, null, 2); // Show full unknown message
                        break;
                }
            } catch (e) {
                logMessage(`Received non-JSON message or parse error: ${rawMessageData}`, 'system');
                console.warn("Failed to parse message as JSON or unknown structure:", e, rawMessageData);
                latestGameEventOutput.textContent = `Error parsing message or non-JSON content:\n${rawMessageData}`;
            }
        };

        webSocket.onerror = (errorEvent) => {
            logMessage(`WebSocket Error. Check browser console for details.`, 'error');
            console.error('WebSocket Error Event:', errorEvent);
            // updateUIDisconnected(); // onclose will handle this
        };

        webSocket.onclose = (event) => {
            let reason = '';
            if (event.code) reason += `Code: ${event.code}`;
            if (event.reason) reason += ` Reason: ${event.reason}`;
            if (!reason && !event.wasClean) reason = 'Connection failed or closed unexpectedly.';
            else if (!reason && event.wasClean) reason = 'Connection closed cleanly.';

            logMessage(`WebSocket disconnected from lobby ${currentLobbyId || 'N/A'}. ${reason}`, event.wasClean ? 'system' : 'error');
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
     * Sends a generic ClientToServerMessage.
     * @param {object} messageObject - The full ClientToServerMessage object.
     */
    function sendGenericToServer(messageObject) {
        if (webSocket && webSocket.readyState === WebSocket.OPEN) {
            const jsonMessage = JSON.stringify(messageObject);
            logMessage(`Sending Generic: ${jsonMessage}`, 'sent');
            webSocket.send(jsonMessage);
        } else {
            logMessage('WebSocket is not connected. Cannot send message.', 'error');
        }
    }


    /**
         * Sends a GameSpecificCommand to the WebSocket server.
         * @param {string} gameTypeId - The game_type_id for routing.
         * @param {object} fullCommandData - The complete game-specific command data object
         *                                   structured as expected by the server's Serde deserialization.
         */
    function sendGameSpecificCommandToServer(gameTypeId, fullCommandData) {
        const messageToSend = {
            message_type: "GameSpecificCommand",
            payload: {
                game_type_id: gameTypeId,
                command_data: fullCommandData // fullCommandData IS the correctly structured command_data
            }
        };
        sendGenericToServer(messageToSend);
    }

    globalEchoBtn.addEventListener('click', () => {
        const userMessage = globalMessageInput.value.trim();
        if (userMessage === "") {
            logMessage("Please type a message for Global Echo.", "error");
            globalMessageInput.focus();
            return;
        }
        const globalCommand = {
            message_type: "GlobalCommand",
            payload: {
                command_name: "Echo", // Example global command name
                data: { message: userMessage } // Payload for the global command
            }
        };
        sendGenericToServer(globalCommand);
    });

    updateUIDisconnected(); // Initial UI state
});

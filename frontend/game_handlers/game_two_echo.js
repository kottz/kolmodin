// game_handlers/game_two_echo.js

(function() {
    const GAME_TYPE_ID = "GameTwoEcho";

    function initUI(controlsContainer, sendCommandToServer) {
        controlsContainer.innerHTML = ''; // Clear previous controls

        const messageInput = document.createElement('input');
        messageInput.type = 'text';
        messageInput.id = `${GAME_TYPE_ID}_messageInput`;
        messageInput.placeholder = 'Message for GameTwoEcho...';
        controlsContainer.appendChild(messageInput);

        const commands = [
            { name: "Echo", commandTag: "Echo" }, // Use commandTag to avoid confusion
            { name: "Send to Self", commandTag: "SendToSelf" },
            { name: "Broadcast All", commandTag: "BroadcastAll" }
        ];

        commands.forEach(cmdInfo => {
            const btn = document.createElement('button');
            btn.textContent = `${cmdInfo.name} (G2)`;
            btn.onclick = () => {
                const msg = messageInput.value.trim();
                // All these commands in GameTwoCommand require a 'message' field.
                if (msg === "") {
                    // You might want to alert or log differently if a message is required
                    console.warn(`Message input is empty for GameTwoEcho ${cmdInfo.name}. Sending empty message.`);
                    // Or:
                    // alert(`Please enter a message for ${cmdInfo.name}.`);
                    // return;
                }

                // Construct the command_data object EXACTLY as Serde expects it
                const commandDataForServer = {
                    command: cmdInfo.commandTag, // This is the 'tag'
                    message: msg                 // This is the field of the enum variant
                };

                sendCommandToServer(GAME_TYPE_ID, commandDataForServer);
            };
            controlsContainer.appendChild(btn);
        });
    }

    function handleGameEvent(eventData, latestEventOutputContainer) {
        latestEventOutputContainer.textContent = JSON.stringify(eventData, null, 2);
    }

    if (window.gameHandlers) {
        window.gameHandlers[GAME_TYPE_ID] = {
            initUI,
            handleGameEvent
        };
    } else {
        console.error("Main game handler registry not found.");
    }
})();

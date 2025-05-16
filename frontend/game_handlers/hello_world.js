// game_handlers/hello_world.js

(function() {
    const GAME_TYPE_ID = "HelloWorldGame";

    function initUI(controlsContainer, sendCommandToServer) {
        controlsContainer.innerHTML = ''; // Clear previous controls

        const messageInput = document.createElement('input');
        messageInput.type = 'text';
        messageInput.id = `${GAME_TYPE_ID}_messageInput`;
        messageInput.placeholder = 'Message for HelloWorldGame...';
        controlsContainer.appendChild(messageInput);

        const commands = [
            { name: "Echo", commandTag: "Echo" },
            { name: "Send to Self", commandTag: "SendToSelf" },
            { name: "Broadcast All", commandTag: "BroadcastAll" }
        ];

        commands.forEach(cmdInfo => {
            const btn = document.createElement('button');
            btn.textContent = `${cmdInfo.name} (HW)`;
            btn.onclick = () => {
                const msg = messageInput.value.trim();
                // if (msg === "") {
                //     alert(`Please enter a message for ${cmdInfo.name}.`);
                //     return;
                // }

                const commandDataForServer = {
                    command: cmdInfo.commandTag,
                    message: msg
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
        console.error("Main game handler registry not found. Ensure script.js loads first and initializes window.gameHandlers.");
    }
})();

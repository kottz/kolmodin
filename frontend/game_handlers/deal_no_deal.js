// game_handlers/deal_no_deal.js

(function() {
    const GAME_TYPE_ID = "DealNoDeal";

    function initUI(controlsContainer, sendCommandToServer) {
        controlsContainer.innerHTML = ''; // Clear previous controls

        const adminCommands = [
            { name: "Start Game", command: "StartGame", payload: {} },
            { name: "Start Player Case Vote", command: "StartPlayerCaseSelectionVote", payload: {} },
            { name: "Start Round Opening Vote", command: "StartRoundCaseOpeningVote", payload: {} },
            { name: "Start Deal/NoDeal Vote", command: "StartDealNoDealVote", payload: {} },
            { name: "Conclude Voting", command: "ConcludeVotingAndProcess", payload: {} }
        ];

        adminCommands.forEach(cmdInfo => {
            const btn = document.createElement('button');
            btn.textContent = `${cmdInfo.name} (DND)`;
            btn.onclick = () => {
                // DND commands often don't need additional input from this simple UI
                sendCommandToServer(GAME_TYPE_ID, { command: cmdInfo.command }, cmdInfo.payload);
            };
            controlsContainer.appendChild(btn);
        });
         const note = document.createElement('p');
        note.textContent = "Twitch chat votes for cases/decisions. Admins use these buttons to progress the game.";
        note.style.fontStyle = "italic";
        note.style.fontSize = "0.9em";
        controlsContainer.appendChild(note);
    }

    function handleGameEvent(eventData, latestEventOutputContainer) {
        // eventData is the DNDGameEvent (e.g., { event_type: "GameStateUpdate", data: { ... } })
        latestEventOutputContainer.textContent = JSON.stringify(eventData, null, 2);

        // You can add more specific UI updates here based on eventData.event_type
        // For example, if eventData.data (the DNDFullGameState) is received,
        // you could parse it and display briefcase values, remaining money, etc.,
        // in a more user-friendly way than just raw JSON.
        // For this "debug UI", just showing the JSON is the primary goal.
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

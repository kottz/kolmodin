// game_handlers/deal_no_deal.js

(function() {
    const GAME_TYPE_ID = "DealNoDeal"; // Must match server's GAME_TYPE_ID_DND

    // --- DOM Element References (cache them for performance if updating frequently) ---
    let gameStateDisplay = null;
    let briefcasesContainer = null;
    let moneyBoardContainer = null;
    let voteTallyContainer = null;
    let liveVoteFeedContainer = null; // For PlayerVoteRegistered events
    let playerCaseDisplay = null;
    let offerDisplay = null;
    let gamePhaseDisplay = null;
    let roundInfoDisplay = null;


    function createElements(controlsContainer) {
        // Create dedicated areas for different parts of the game UI
        gamePhaseDisplay = document.createElement('div');
        gamePhaseDisplay.id = 'dnd-game-phase';
        controlsContainer.appendChild(gamePhaseDisplay);

        roundInfoDisplay = document.createElement('div');
        roundInfoDisplay.id = 'dnd-round-info';
        controlsContainer.appendChild(roundInfoDisplay);

        playerCaseDisplay = document.createElement('div');
        playerCaseDisplay.id = 'dnd-player-case';
        controlsContainer.appendChild(playerCaseDisplay);

        offerDisplay = document.createElement('div');
        offerDisplay.id = 'dnd-banker-offer';
        controlsContainer.appendChild(offerDisplay);
        
        const boardAndVotes = document.createElement('div');
        boardAndVotes.style.display = 'flex';
        boardAndVotes.style.gap = '20px';

        const leftPanel = document.createElement('div');
        leftPanel.style.flex = '1';

        briefcasesContainer = document.createElement('div');
        briefcasesContainer.id = 'dnd-briefcases';
        briefcasesContainer.style.display = 'grid';
        briefcasesContainer.style.gridTemplateColumns = 'repeat(auto-fill, minmax(80px, 1fr))';
        briefcasesContainer.style.gap = '10px';
        leftPanel.appendChild(briefcasesContainer);
        
        voteTallyContainer = document.createElement('div');
        voteTallyContainer.id = 'dnd-vote-tally';
        leftPanel.appendChild(voteTallyContainer);

        const rightPanel = document.createElement('div');
        rightPanel.style.flex = '1';
        rightPanel.style.maxWidth = '300px';


        moneyBoardContainer = document.createElement('div');
        moneyBoardContainer.id = 'dnd-money-board';
        rightPanel.appendChild(moneyBoardContainer);

        liveVoteFeedContainer = document.createElement('ul'); // Use a list for the feed
        liveVoteFeedContainer.id = 'dnd-live-vote-feed';
        liveVoteFeedContainer.style.maxHeight = '200px';
        liveVoteFeedContainer.style.overflowY = 'auto';
        live_vote_feed_title = document.createElement('h4');
        live_vote_feed_title.textContent = "Live Valid Votes:";
        rightPanel.appendChild(live_vote_feed_title);
        rightPanel.appendChild(liveVoteFeedContainer);


        boardAndVotes.appendChild(leftPanel);
        boardAndVotes.appendChild(rightPanel);
        controlsContainer.appendChild(boardAndVotes);


        // General game state JSON display (for debugging)
        const debugTitle = document.createElement('h3');
        debugTitle.textContent = "Raw Game State (Debug):";
        controlsContainer.appendChild(debugTitle);
        gameStateDisplay = document.createElement('pre');
        gameStateDisplay.id = 'dnd-game-state-json';
        controlsContainer.appendChild(gameStateDisplay);
    }


    function initUI(controlsContainer, sendCommandToServer) {
        controlsContainer.innerHTML = ''; // Clear previous controls

        // Admin command buttons
        const commandButtonContainer = document.createElement('div');
        commandButtonContainer.style.marginBottom = '20px';

        const adminCommands = [
            // Commands match the simplified AdminCommand enum on the server
            { name: "Start Game", command: "StartGame" },
            { name: "Start Voting", command: "StartVoting" },
            { name: "Conclude Voting & Process", command: "ConcludeVotingAndProcess" }
        ];

        adminCommands.forEach(cmdInfo => {
            const btn = document.createElement('button');
            btn.textContent = cmdInfo.name; // No "(DND)" needed as this handler is DND specific
            btn.onclick = () => {
                // These commands typically don't need a payload from this simple UI
                sendCommandToServer(GAME_TYPE_ID, { command: cmdInfo.command }, {});
            };
            commandButtonContainer.appendChild(btn);
        });
        controlsContainer.appendChild(commandButtonContainer);

        const note = document.createElement('p');
        note.textContent = "Twitch chat votes (1-22 for cases, 'Deal'/'No Deal'). Admins use buttons to progress.";
        note.style.fontStyle = "italic";
        note.style.fontSize = "0.9em";
        controlsContainer.appendChild(note);

        // Create UI elements for displaying game information
        createElements(controlsContainer);
    }

    function updateBriefcaseDisplay(gameState) {
        if (!briefcasesContainer) return;
        briefcasesContainer.innerHTML = '';

        gameState.briefcase_values.forEach((value, index) => {
            const caseEl = document.createElement('div');
            caseEl.classList.add('briefcase');
            caseEl.style.border = '1px solid #ccc';
            caseEl.style.padding = '10px';
            caseEl.style.textAlign = 'center';
            caseEl.style.minHeight = '60px'; // Ensure consistent height
             caseEl.style.display = 'flex';
            caseEl.style.flexDirection = 'column';
            caseEl.style.justifyContent = 'center';
            caseEl.style.alignItems = 'center';


            const caseNumber = index + 1; // Display 1-based numbers
            caseEl.textContent = `#${caseNumber}`;

            if (gameState.briefcase_is_opened[index]) {
                caseEl.style.backgroundColor = '#e0e0e0';
                caseEl.innerHTML += `<br>$${value.toLocaleString()}`; // Show value if opened
            } else {
                caseEl.style.backgroundColor = '#f0f0f0';
            }

            if (gameState.player_chosen_case_index === index) {
                caseEl.style.borderColor = 'gold';
                caseEl.style.borderWidth = '3px';
                caseEl.innerHTML += `<br>(Player's Case)`;
            }
            briefcasesContainer.appendChild(caseEl);
        });
    }

    function updateMoneyBoard(gameState) {
        if (!moneyBoardContainer) return;
        moneyBoardContainer.innerHTML = '<h4>Money Board</h4>';
        
        // Assuming MONEY_VALUES is available globally or passed appropriately
        // For this example, let's define it here for simplicity matching server
        const ALL_MONEY_VALUES = [
            1, 5, 10, 25, 50, 75, 100, 200, 300, 400, 500, 750, 1000, 5000, 10000, 25000, 50000,
            75000, 100000, 250000, 500000, 1000000,
        ].sort((a,b) => a-b);


        ALL_MONEY_VALUES.forEach(value => {
            const moneyEl = document.createElement('div');
            moneyEl.textContent = `$${value.toLocaleString()}`;
            if (!gameState.remaining_money_values_in_play.includes(value)) {
                moneyEl.style.textDecoration = 'line-through';
                moneyEl.style.color = '#aaa';
            } else {
                moneyEl.style.fontWeight = 'bold';
                 // Highlight big values
                if (value >= 50000) moneyEl.style.color = 'green';
                if (value >= 250000) moneyEl.style.color = 'orange';
                if (value >= 1000000) moneyEl.style.color = 'red';

            }
            moneyBoardContainer.appendChild(moneyEl);
        });
    }

    function updateVoteTallyDisplay(gameState) {
        if (!voteTallyContainer) return;
        voteTallyContainer.innerHTML = ''; // Clear previous tally

        if (gameState.current_vote_tally && Object.keys(gameState.current_vote_tally).length > 0) {
            const title = document.createElement('h4');
            title.textContent = 'Current Vote Tally:';
            voteTallyContainer.appendChild(title);

            const ul = document.createElement('ul');
            for (const [option, count] of Object.entries(gameState.current_vote_tally)) {
                const li = document.createElement('li');
                li.textContent = `${option}: ${count}`;
                ul.appendChild(li);
            }
            voteTallyContainer.appendChild(ul);
        } else if (gameState.phase.endsWith("_Voting")) { // If in voting phase but no votes yet
             const title = document.createElement('h4');
            title.textContent = 'Current Vote Tally:';
            voteTallyContainer.appendChild(title);
            const p = document.createElement('p');
            p.textContent = "Awaiting votes...";
            voteTallyContainer.appendChild(p);
        }
    }
    
    function updateGameInfoDisplays(gameState) {
        if (gamePhaseDisplay) {
            gamePhaseDisplay.innerHTML = `<h3>Phase: ${gameState.phase.replace(/_/g, ' ')}</h3>`;
             if (gameState.phase.startsWith("GameOver") && gameState.phase.summary) {
                gamePhaseDisplay.innerHTML += `<p><em>${gameState.phase.summary}</em></p>`;
            }
        }
        if (roundInfoDisplay) {
            if (gameState.phase.includes("Round") || gameState.phase.includes("DealOrNoDeal")) {
                 roundInfoDisplay.textContent = `Round: ${gameState.current_round_display_number} | Cases to open this round: ${gameState.cases_to_open_this_round_target} | Opened so far: ${gameState.cases_opened_in_current_round_segment}`;
            } else {
                roundInfoDisplay.textContent = '';
            }
        }

        if (playerCaseDisplay) {
            if (gameState.player_chosen_case_index !== null && gameState.player_chosen_case_index !== undefined) {
                const playerCaseNumber = gameState.player_chosen_case_index + 1;
                let text = `Player's Chosen Case: #${playerCaseNumber}`;
                // If game is over and it was a no deal, or if player case is opened.
                if (gameState.briefcase_is_opened[gameState.player_chosen_case_index]) {
                     text += ` (Value: $${gameState.briefcase_values[gameState.player_chosen_case_index].toLocaleString()})`;
                }
                playerCaseDisplay.textContent = text;
            } else {
                playerCaseDisplay.textContent = "Player's Case: Not chosen yet.";
            }
        }
        if (offerDisplay) {
            if (gameState.banker_offer !== null && gameState.banker_offer !== undefined) {
                offerDisplay.textContent = `Banker's Offer: $${gameState.banker_offer.toLocaleString()}`;
            } else {
                offerDisplay.textContent = '';
            }
        }
    }


    function handleFullStateUpdate(gameState) {
        // gameState is the full GameState object from the server
        if (gameStateDisplay) {
            gameStateDisplay.textContent = JSON.stringify(gameState, null, 2); // For debugging
        }
        
        updateBriefcaseDisplay(gameState);
        updateMoneyBoard(gameState);
        updateVoteTallyDisplay(gameState); // Tally updates with full state
        updateGameInfoDisplays(gameState);
        
        // Clear live vote feed on full state update, as tally is now synced
        if (liveVoteFeedContainer) liveVoteFeedContainer.innerHTML = '';
    }

    function handlePlayerVoteRegistered(data) {
        // data = { voter_username: "...", vote_value: "..." }
        if (liveVoteFeedContainer) {
            const li = document.createElement('li');
            li.textContent = `${data.voter_username}: ${data.vote_value}`;
            liveVoteFeedContainer.prepend(li); // Add to top of list

            // Optional: Limit the number of displayed votes to prevent overflow
            while (liveVoteFeedContainer.children.length > 20) { // Keep last 20 votes
                liveVoteFeedContainer.removeChild(liveVoteFeedContainer.lastChild);
            }
        }
         // Note: The main vote tally display is NOT updated here.
        // It only updates on FullStateUpdate to keep this event lightweight.
    }

    function handleCaseOpened(data) {
        // data = { case_index: ..., value: ..., is_player_case_reveal_at_end: ... }
        console.log(`Case Opened Event: Case index ${data.case_index + 1} contained $${data.value}`);
        // UI could animate this specific case opening.
        // For now, the next FullStateUpdate will refresh the board.
        // Or, you could find the specific case element and update it immediately.
         const caseEl = briefcasesContainer ? briefcasesContainer.children[data.case_index] : null;
        if (caseEl) {
            caseEl.style.backgroundColor = '#e0e0e0'; // Visually mark as opened
            // Check if player's case text needs update
            let currentText = caseEl.textContent;
            if(currentText.includes("(Player's Case)")){
                 caseEl.innerHTML = `#${data.case_index + 1}<br>$${data.value.toLocaleString()}<br>(Player's Case)`;
            } else {
                 caseEl.innerHTML = `#${data.case_index + 1}<br>$${data.value.toLocaleString()}`;
            }
        }
    }

    function handleBankerOfferPresented(data) {
        // data = { offer_amount: ... }
        console.log(`Banker Offer Event: $${data.offer_amount}`);
        if (offerDisplay) {
            offerDisplay.textContent = `Banker's Offer: $${data.offer_amount.toLocaleString()}`;
        }
        // The next FullStateUpdate will also contain this offer in GameState.banker_offer.
    }


    // Main event handler function called by the game engine
    function handleGameEvent(eventData, latestEventOutputContainer /* unused */) {
        // eventData is the GameEvent (e.g., { event_type: "FullStateUpdate", data: { ... } })
        
        // For the general debug display, we can still show the raw event
        if (gameStateDisplay) { // If gameStateDisplay is used for raw events
             const rawEventDiv = document.createElement('div');
             rawEventDiv.textContent = `EVENT (${eventData.event_type}): ${JSON.stringify(eventData.data, null, 2)}`;
             rawEventDiv.style.borderBottom = "1px dashed #ccc";
             rawEventDiv.style.marginBottom = "5px";
             // gameStateDisplay.prepend(rawEventDiv); // Prepend to see latest first
        }


        switch (eventData.event_type) {
            case "FullStateUpdate":
                handleFullStateUpdate(eventData.data);
                break;
            case "PlayerVoteRegistered":
                handlePlayerVoteRegistered(eventData.data);
                break;
            case "CaseOpened":
                handleCaseOpened(eventData.data);
                // A full state update usually follows quickly or is not strictly needed if UI patches this
                break;
            case "BankerOfferPresented":
                handleBankerOfferPresented(eventData.data);
                // A full state update will follow to confirm phase change and offer
                break;
            default:
                console.warn("DND Handler: Received unknown event_type:", eventData.event_type);
                // Display unknown events in the raw log too
                if(gameStateDisplay) gameStateDisplay.textContent = `UNKNOWN EVENT: ${JSON.stringify(eventData, null, 2)}`;
        }
    }

    // Register this handler with the main game engine
    if (window.gameHandlers) {
        window.gameHandlers[GAME_TYPE_ID] = {
            initUI,
            handleGameEvent
        };
    } else {
        console.error("Main game handler registry not found.");
    }
})();

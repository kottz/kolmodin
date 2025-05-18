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
            // { name: "Start Voting", command: "StartVoting" }, // REMOVED - Voting starts automatically
            { name: "Conclude Voting & Process", command: "ConcludeVotingAndProcess" }
        ];

        adminCommands.forEach(cmdInfo => {
            const btn = document.createElement('button');
            btn.textContent = cmdInfo.name;
            btn.onclick = () => {
                sendCommandToServer(GAME_TYPE_ID, { command: cmdInfo.command }, {});
            };
            commandButtonContainer.appendChild(btn);
        });
        controlsContainer.appendChild(commandButtonContainer);

        const note = document.createElement('p');
        note.textContent = "Twitch chat votes (1-22 for cases, 'Deal'/'No Deal'). Admin uses 'Conclude Voting' to process votes and move to the next step.";
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
            caseEl.style.minHeight = '60px';
            caseEl.style.display = 'flex';
            caseEl.style.flexDirection = 'column';
            caseEl.style.justifyContent = 'center';
            caseEl.style.alignItems = 'center';


            const caseNumber = index + 1;
            let caseHTML = `#${caseNumber}`;

            if (gameState.briefcase_is_opened[index]) {
                caseEl.style.backgroundColor = '#e0e0e0';
                caseHTML += `<br>$${value.toLocaleString()}`;
            } else {
                caseEl.style.backgroundColor = '#f0f0f0';
            }

            if (gameState.player_chosen_case_index === index) {
                caseEl.style.borderColor = 'gold';
                caseEl.style.borderWidth = '3px';
                if (!gameState.briefcase_is_opened[index]) { // Only add (Player's Case) text if not already showing value
                    caseHTML += `<br>(Player's Case)`;
                } else if (gameState.phase.startsWith("GameOver")) { // If game over, explicitly show it was player's case
                    caseHTML += `<br>(Player's Case)`;
                }
            }
            caseEl.innerHTML = caseHTML;
            briefcasesContainer.appendChild(caseEl);
        });
    }

    function updateMoneyBoard(gameState) {
        if (!moneyBoardContainer) return;
        moneyBoardContainer.innerHTML = '<h4>Money Board</h4>';

        const ALL_MONEY_VALUES = [
            1, 5, 10, 25, 50, 75, 100, 200, 300, 400, 500, 750, 1000, 5000, 10000, 25000, 50000,
            75000, 100000, 250000, 500000, 1000000,
        ].sort((a, b) => a - b);


        ALL_MONEY_VALUES.forEach(value => {
            const moneyEl = document.createElement('div');
            moneyEl.textContent = `$${value.toLocaleString()}`;
            if (!gameState.remaining_money_values_in_play.includes(value)) {
                moneyEl.style.textDecoration = 'line-through';
                moneyEl.style.color = '#aaa';
            } else {
                moneyEl.style.fontWeight = 'bold';
                if (value >= 50000) moneyEl.style.color = 'green';
                if (value >= 250000) moneyEl.style.color = 'orange';
                if (value >= 1000000) moneyEl.style.color = 'red';

            }
            moneyBoardContainer.appendChild(moneyEl);
        });
    }

    function updateVoteTallyDisplay(gameState) {
        if (!voteTallyContainer) return;
        voteTallyContainer.innerHTML = '';

        const currentPhaseString = typeof gameState.phase === 'string' ? gameState.phase : gameState.phase.type || Object.keys(gameState.phase)[0];


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
        } else if (currentPhaseString && currentPhaseString.endsWith("_Voting")) {
            const title = document.createElement('h4');
            title.textContent = 'Current Vote Tally:';
            voteTallyContainer.appendChild(title);
            const p = document.createElement('p');
            p.textContent = "Awaiting votes...";
            voteTallyContainer.appendChild(p);
        }
    }

    function updateGameInfoDisplays(gameState) {
        const currentPhaseString = typeof gameState.phase === 'string' ? gameState.phase : gameState.phase.type || Object.keys(gameState.phase)[0];

        if (gamePhaseDisplay) {
            let phaseText = currentPhaseString.replace(/_/g, ' ');
            if (gameState.phase.data && gameState.phase.data.round_number) { // For new enum style
                phaseText += ` (Round ${gameState.phase.data.round_number})`;
            } else if (typeof gameState.phase === 'object' && gameState.phase.round_number) { // For old style if it slips
                phaseText += ` (Round ${gameState.phase.round_number})`;
            } else if (gameState.phase.GameOver && gameState.phase.GameOver.summary) { // For GameOver
                phaseText = `Game Over`; // Main title
                gamePhaseDisplay.innerHTML = `<h3>${phaseText}</h3><p><em>${gameState.phase.GameOver.summary}</em></p>`;
                // return early or ensure other displays are cleared/updated appropriately for game over
            } else if (currentPhaseString === "GameOver" && gameState.phase.summary) { // For older GameOver string variant
                phaseText = `Game Over`;
                gamePhaseDisplay.innerHTML = `<h3>${phaseText}</h3><p><em>${gameState.phase.summary}</em></p>`;
            }


            if (!gamePhaseDisplay.innerHTML.includes("Game Over")) { // Avoid overwriting game over summary
                gamePhaseDisplay.innerHTML = `<h3>Phase: ${phaseText}</h3>`;
            }
        }

        if (roundInfoDisplay) {
            if (currentPhaseString.includes("Round") || currentPhaseString.includes("DealOrNoDeal") || currentPhaseString.includes("BankerOfferCalculation")) {
                roundInfoDisplay.textContent = `Round: ${gameState.current_round_display_number} | Cases to open this round: ${gameState.cases_to_open_this_round_target} | Opened so far: ${gameState.cases_opened_in_current_round_segment}`;
            } else if (currentPhaseString === "GameOver") {
                roundInfoDisplay.textContent = `Final Winnings: $${gameState.banker_offer !== null ? gameState.banker_offer.toLocaleString() : 'N/A'}`; // banker_offer holds winnings in GameOver
            }
            else {
                roundInfoDisplay.textContent = '';
            }
        }

        if (playerCaseDisplay) {
            if (gameState.player_chosen_case_index !== null && gameState.player_chosen_case_index !== undefined) {
                const playerCaseNumber = gameState.player_chosen_case_index + 1;
                let text = `Player's Chosen Case: #${playerCaseNumber}`;
                if (gameState.briefcase_is_opened[gameState.player_chosen_case_index]) {
                    text += ` (Value: $${gameState.briefcase_values[gameState.player_chosen_case_index].toLocaleString()})`;
                }
                playerCaseDisplay.textContent = text;
            } else {
                playerCaseDisplay.textContent = "Player's Case: Not chosen yet.";
            }
        }
        if (offerDisplay) {
            if (gameState.banker_offer !== null && gameState.banker_offer !== undefined && currentPhaseString !== "GameOver") {
                offerDisplay.textContent = `Banker's Offer: $${gameState.banker_offer.toLocaleString()}`;
            } else {
                offerDisplay.textContent = ''; // Clear offer if not game over and no current offer
            }
        }
    }


    function handleFullStateUpdate(gameState) {
        if (gameStateDisplay) {
            gameStateDisplay.textContent = JSON.stringify(gameState, null, 2);
        }

        updateBriefcaseDisplay(gameState);
        updateMoneyBoard(gameState);
        updateVoteTallyDisplay(gameState);
        updateGameInfoDisplays(gameState);

        if (liveVoteFeedContainer) liveVoteFeedContainer.innerHTML = '';
    }

    function handlePlayerVoteRegistered(data) {
        if (liveVoteFeedContainer) {
            const li = document.createElement('li');
            li.textContent = `${data.voter_username}: ${data.vote_value}`;
            liveVoteFeedContainer.prepend(li);

            while (liveVoteFeedContainer.children.length > 20) {
                liveVoteFeedContainer.removeChild(liveVoteFeedContainer.lastChild);
            }
        }
    }

    function handleCaseOpened(data) {
        console.log(`Case Opened Event: Case index ${data.case_index + 1} contained $${data.value}`);
        // This provides immediate feedback. FullStateUpdate will solidify it.
        const caseEl = briefcasesContainer ? briefcasesContainer.children[data.case_index] : null;
        if (caseEl) {
            caseEl.style.backgroundColor = '#e0e0e0';

            let newHTML = `#${data.case_index + 1}<br>$${data.value.toLocaleString()}`;
            // Check if it was the player's case using the existing display text (a bit fragile but works for quick update)
            // A more robust way would be to check against gameState.player_chosen_case_index if the full gameState was available here
            // or if this event carried that info. For now, this approximates.
            if (caseEl.innerHTML.includes("(Player's Case)")) {
                newHTML += `<br>(Player's Case)`;
            }
            caseEl.innerHTML = newHTML;
        }
    }

    function handleBankerOfferPresented(data) {
        console.log(`Banker Offer Event: $${data.offer_amount}`);
        if (offerDisplay) {
            offerDisplay.textContent = `Banker's Offer: $${data.offer_amount.toLocaleString()}`;
        }
    }


    function handleGameEvent(eventData, latestEventOutputContainer) {
        switch (eventData.event_type) {
            case "FullStateUpdate":
                handleFullStateUpdate(eventData.data);
                break;
            case "PlayerVoteRegistered":
                handlePlayerVoteRegistered(eventData.data);
                break;
            case "CaseOpened":
                handleCaseOpened(eventData.data);
                // Note: FullStateUpdate typically follows server-side after such actions.
                // If not, you might need to request a state update or make more comprehensive local UI patches.
                break;
            case "BankerOfferPresented":
                handleBankerOfferPresented(eventData.data);
                // Similar to CaseOpened, a FullStateUpdate is expected to follow.
                break;
            default:
                console.warn("DND Handler: Received unknown event_type:", eventData.event_type, eventData.data);
                if (gameStateDisplay) gameStateDisplay.textContent = `UNKNOWN EVENT (${eventData.event_type}): ${JSON.stringify(eventData.data, null, 2)}`;
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

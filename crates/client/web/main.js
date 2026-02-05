// Connection injection points for servers
// COGAR_CONNECTION_INJECT_POINT
// CIGAR_CONNECTION_INJECT_POINT

import init, { GameClientWrapper } from './pkg/client.js';

let gameClient = null;
let availableServers = [];
let selectedServerUrl = null;
let availableSkins = [];
let selectedSkin = '';

// Determine available servers based on context
function getAvailableServers() {
    // Check if cogar injected a direct connection
    if (window.COGAR_CONNECTION) {
        return [{ url: normalizeUrl(window.COGAR_CONNECTION), name: 'Current Server' }];
    }
    
    // Check if cigar injected connection options
    if (window.CIGAR_CONNECTIONS && Array.isArray(window.CIGAR_CONNECTIONS) && window.CIGAR_CONNECTIONS.length > 0) {
        return window.CIGAR_CONNECTIONS.map(conn => ({
            url: normalizeUrl(conn.url),
            name: conn.name || conn.url
        }));
    }
    
    // Fallback to query params or default
    const params = new URLSearchParams(window.location.search);
    const serverUrl = params.get('server') || '/game';
    return [{ url: normalizeUrl(serverUrl), name: serverUrl }];
}

// Normalize URL format for WebSocket connection
function normalizeUrl(url) {
    // Handle relative paths (like "/game") 
    if (url.startsWith('/')) {
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        return `${protocol}//${window.location.host}${url}`;
    }
    
    // Handle URLs without protocol
    if (!url.startsWith('ws://') && !url.startsWith('wss://')) {
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        return `${protocol}//${url}`;
    }
    
    return url;
}

// Show server selection UI
function showServerSelection() {
    const serverSelection = document.getElementById('serverSelection');
    const serverList = document.getElementById('serverList');
    const loginOverlay = document.getElementById('loginOverlay');
    
    console.log('Showing server selection UI with', availableServers.length, 'servers');
    
    serverList.innerHTML = '';
    
    availableServers.forEach((server, index) => {
        const button = document.createElement('button');
        button.className = 'py-3 px-6 text-lg border border-white/30 rounded text-white bg-white/10 hover:bg-white/20 transition-colors min-w-64';
        button.textContent = server.name;
        button.addEventListener('click', () => {
            console.log('Server selected:', server.name, server.url);
            selectedServerUrl = server.url;
            hideServerSelection();
            showLoginOverlay();
            initializeGameClient();
        });
        serverList.appendChild(button);
    });
    
    serverSelection.classList.remove('hidden');
    serverSelection.classList.add('flex', 'flex-col', 'items-center', 'justify-center');
    loginOverlay.classList.add('hidden');
    loginOverlay.classList.remove('flex', 'flex-col', 'items-center', 'justify-center');
    document.body.classList.add('overlay-visible');
}

// Hide server selection UI
function hideServerSelection() {
    const serverSelection = document.getElementById('serverSelection');
    serverSelection.classList.add('hidden');
    serverSelection.classList.remove('flex', 'flex-col', 'items-center', 'justify-center');
}

// Show login overlay
function showLoginOverlay() {
    const loginOverlay = document.getElementById('loginOverlay');
    loginOverlay.classList.remove('hidden');
    loginOverlay.classList.add('flex', 'flex-col', 'items-center', 'justify-center');
    document.body.classList.add('overlay-visible');
}

// Hide login overlay
function hideLoginOverlay() {
    const loginOverlay = document.getElementById('loginOverlay');
    loginOverlay.classList.add('hidden');
    loginOverlay.classList.remove('flex', 'flex-col', 'items-center', 'justify-center');
    document.body.classList.remove('overlay-visible');
}

// Fetch available skins from server
async function fetchSkins() {
    try {
        const response = await fetch('skinList.txt');
        if (!response.ok) {
            throw new Error('Failed to fetch skins');
        }
        const text = await response.text();
        // skinList.txt contains comma-separated skin names
        availableSkins = text.trim().split(',').map(s => s.trim()).filter(s => s);
        return availableSkins;
    } catch (error) {
        console.error('Error fetching skins:', error);
        return [];
    }
}

// Show skin picker modal
async function showSkinPicker() {
    const modal = document.getElementById('skinPickerModal');
    const loading = document.getElementById('skinPickerLoading');
    const grid = document.getElementById('skinGrid');
    const error = document.getElementById('skinPickerError');

    modal.classList.remove('hidden');
    modal.classList.add('flex', 'items-center', 'justify-center');
    document.body.classList.add('overlay-visible');
    loading.classList.remove('hidden');
    grid.classList.add('hidden');
    error.classList.add('hidden');

    const skins = await fetchSkins();

    if (skins.length === 0) {
        loading.classList.add('hidden');
        error.classList.remove('hidden');
        return;
    }

    loading.classList.add('hidden');
    grid.classList.remove('hidden');

    // Populate skin grid
    grid.innerHTML = '';
    skins.forEach(skinName => {
        const skinItem = document.createElement('div');
        skinItem.className = 'skin-item';
        if (selectedSkin === skinName) {
            skinItem.classList.add('selected');
        }

        // Try webp first, fallback to png
        const img = document.createElement('img');
        img.alt = skinName;
        img.title = skinName;
        
        // Try loading webp first
        img.src = `skins/${skinName}.webp`;
        img.onerror = () => {
            // Fallback to png
            img.src = `skins/${skinName}.png`;
            img.onerror = () => {
                // If both fail, show placeholder
                img.style.display = 'none';
            };
        };

        const nameDiv = document.createElement('div');
        nameDiv.className = 'skin-item-name';
        nameDiv.textContent = skinName;

        skinItem.appendChild(img);
        skinItem.appendChild(nameDiv);

        skinItem.addEventListener('click', () => {
            selectedSkin = skinName;
            document.getElementById('skinInput').value = skinName;
            
            // Update selected state
            grid.querySelectorAll('.skin-item').forEach(item => {
                item.classList.remove('selected');
            });
            skinItem.classList.add('selected');
            
            // Close modal after short delay
            setTimeout(() => {
                hideSkinPicker();
            }, 200);
        });

        grid.appendChild(skinItem);
    });
}

// Hide skin picker modal
function hideSkinPicker() {
    const modal = document.getElementById('skinPickerModal');
    modal.classList.add('hidden');
    modal.classList.remove('flex', 'items-center', 'justify-center');
    document.body.classList.remove('overlay-visible');
}

// Wait for websocket to open (or fail) with a timeout
// Polls readyState directly since the open event may fire before we can listen
function waitForWebSocketOpen(getWs, timeoutMs = 10000) {
    return new Promise((resolve, reject) => {
        const startTime = Date.now();

        function poll() {
            // Check timeout
            if (Date.now() - startTime > timeoutMs) {
                reject(new Error('WebSocket timeout'));
                return;
            }

            const ws = getWs?.();
            if (!ws) {
                // WebSocket not available yet, keep polling
                setTimeout(poll, 50);
                return;
            }

            // Check readyState: 0=CONNECTING, 1=OPEN, 2=CLOSING, 3=CLOSED
            if (ws.readyState === 1) {
                resolve();
                return;
            }
            if (ws.readyState === 2 || ws.readyState === 3) {
                reject(new Error('WebSocket closed'));
                return;
            }

            // Still connecting, poll again
            setTimeout(poll, 50);
        }

        poll();
    });
}

// Initialize the game client with selected server
async function initializeGameClient() {
    if (!selectedServerUrl) return;

    const playButton = document.getElementById('playButton');
    playButton.textContent = 'Connecting...';
    playButton.disabled = true;

    try {
        console.log('Connecting to server:', selectedServerUrl);

        // Create the game client immediately to establish WebSocket connection
        gameClient = new GameClientWrapper('gameCanvas', selectedServerUrl);

        await waitForWebSocketOpen(() => gameClient?.websocket?.(), 10000);

        playButton.disabled = false;
        playButton.textContent = 'Play';
    } catch (error) {
        console.error('Failed to connect to server:', error);
        playButton.textContent = 'Connection Failed';
        playButton.disabled = true;
    }
}

// Death detection - shows overlay when player dies
let deathCheckInterval = null;
let wasAlive = false;

function startDeathDetection() {
    // Clear any existing interval
    if (deathCheckInterval) {
        clearInterval(deathCheckInterval);
        deathCheckInterval = null;
    }

    wasAlive = false;

    deathCheckInterval = setInterval(() => {
        try {
            if (!gameClient) {
                return;
            }

            let cellCount;
            try {
                cellCount = gameClient.cell_count();
            } catch (wasmError) {
                throw wasmError;
            }
            
            const isAlive = cellCount > 0;

            // Player just spawned
            if (isAlive && !wasAlive) {
                wasAlive = true;
            }

            // Player just died (was alive, now dead)
            if (!isAlive && wasAlive) {
                wasAlive = false;

                // Show the login overlay for respawn
                const loginOverlay = document.getElementById('loginOverlay');
                const stats = document.getElementById('stats');
                const leaderboard = document.getElementById('leaderboard');

                loginOverlay.classList.remove('hidden');
                loginOverlay.classList.add('flex', 'flex-col', 'items-center', 'justify-center');
                document.body.classList.add('overlay-visible');
                stats.classList.add('hidden');
                leaderboard.classList.add('hidden');
                // Keep chatBox visible so they can see death messages
            }
        } catch (error) {
            // Re-throw to see if this kills the interval
            throw error;
        }
    }, 500); // Check every 500ms
}

async function run() {
    try {
        // Initialize WASM
        await init();
        console.log('WASM initialized');

        // Hide loading overlay after WASM loads
        const loadingOverlay = document.getElementById('loadingOverlay');
        loadingOverlay.classList.add('loaded');
        // Remove from DOM after fade animation
        setTimeout(() => {
            loadingOverlay.remove();
        }, 300);

        // Set overlay-visible class initially since loginOverlay is visible by default
        document.body.classList.add('overlay-visible');

        // Get available servers
        availableServers = getAvailableServers();
        console.log('Available servers:', availableServers);

        // Setup change server button visibility
        const changeServerButton = document.getElementById('changeServerButton');
        if (availableServers.length > 1) {
            changeServerButton.classList.remove('hidden');
        }

        // If multiple servers, show selection UI
        if (availableServers.length > 1) {
            showServerSelection();
        } else {
            // Single server, connect directly
            selectedServerUrl = availableServers[0].url;
            showLoginOverlay(); // Ensure login is visible
            initializeGameClient();
        }

        // Handle back to server selection button
        const backToLogin = document.getElementById('backToLogin');
        backToLogin.addEventListener('click', () => {
            hideServerSelection();
            showLoginOverlay();
        });
        
        // Handle change server button
        changeServerButton.addEventListener('click', () => {
            if (availableServers.length > 1) {
                document.getElementById('loginOverlay').classList.add('hidden');
                showServerSelection();
            }
        });

        // Handle play button
        const playButton = document.getElementById('playButton');
        const loginOverlay = document.getElementById('loginOverlay');
        const stats = document.getElementById('stats');
        const leaderboard = document.getElementById('leaderboard');
        const chatBox = document.getElementById('chatBox');
        const minimapCanvas = document.getElementById('minimapCanvas');
        const showMinimap = document.getElementById('settingShowMinimap');

        playButton.addEventListener('click', async () => {
            if (!selectedServerUrl) return;

            playButton.disabled = true;
            playButton.textContent = 'Connecting...';

            try {
                const nick = document.getElementById('nickInput').value.trim() || 'Unnamed';
                const skin = document.getElementById('skinInput').value.trim();
                const spawnName = skin ? `{${skin}}${nick}` : nick;

                // If game client doesn't exist yet (multi-server case), create it
                if (!gameClient) {
                    gameClient = new GameClientWrapper('gameCanvas', selectedServerUrl);
                }

                await waitForWebSocketOpen(() => gameClient?.websocket?.(), 5000);

                // Spawn the player
                gameClient.spawn(spawnName);

                // Hide login overlay and show game UI
                loginOverlay.classList.add('hidden');
                loginOverlay.classList.remove('flex', 'flex-col', 'items-center', 'justify-center');
                document.body.classList.remove('overlay-visible');
                stats.classList.remove('hidden');
                leaderboard.classList.remove('hidden');
                chatBox.classList.remove('hidden');
                if (showMinimap?.checked) {
                    minimapCanvas.classList.remove('hidden');
                }

                // Reset button for respawn
                playButton.disabled = false;
                playButton.textContent = 'Play';

                // Start death detection - check if player died and show overlay
                startDeathDetection();
            } catch (error) {
                console.error('Failed to start game:', error);
                alert('Failed to connect to server: ' + error.message);
                playButton.disabled = false;
                playButton.textContent = 'Play';
            }
        });

        // Handle enter key in nick input
        document.getElementById('nickInput').addEventListener('keypress', (e) => {
            if (e.key === 'Enter' && !playButton.disabled) {
                playButton.click();
            }
        });
        document.getElementById('skinInput').addEventListener('keypress', (e) => {
            if (e.key === 'Enter' && !playButton.disabled) {
                playButton.click();
            }
        });

        // Help popup toggle
        const helpBtn = document.getElementById('helpBtn');
        const helpPopup = document.getElementById('helpPopup');
        helpBtn.addEventListener('click', (e) => {
            e.stopPropagation();
            const isOpen = helpPopup.dataset.open === 'true';
            helpPopup.dataset.open = isOpen ? 'false' : 'true';
        });

        // Settings popup toggle
        const settingsBtn = document.getElementById('settingsBtn');
        const settingsPopup = document.getElementById('settingsPopup');
        settingsBtn.addEventListener('click', (e) => {
            e.stopPropagation();
            const isOpen = settingsPopup.dataset.open === 'true';
            settingsPopup.dataset.open = isOpen ? 'false' : 'true';
        });

        // Close popups when clicking outside
        document.addEventListener('click', (e) => {
            if (!helpPopup.contains(e.target) && e.target !== helpBtn) {
                helpPopup.dataset.open = 'false';
            }
            if (!settingsPopup.contains(e.target) && e.target !== settingsBtn) {
                settingsPopup.dataset.open = 'false';
            }
        });

        // Skin picker button
        const skinPickerBtn = document.getElementById('skinPickerBtn');
        const closeSkinPicker = document.getElementById('closeSkinPicker');
        const skinInput = document.getElementById('skinInput');
        
        skinPickerBtn.addEventListener('click', (e) => {
            e.preventDefault();
            showSkinPicker();
        });
        
        closeSkinPicker.addEventListener('click', () => {
            hideSkinPicker();
        });

        // Update selectedSkin when input changes manually
        skinInput.addEventListener('input', (e) => {
            selectedSkin = e.target.value.trim();
        });

        // Show login overlay on Escape
        document.addEventListener('keydown', (e) => {
            if (e.key !== 'Escape') return;
            const loginOverlay = document.getElementById('loginOverlay');
            const stats = document.getElementById('stats');
            const leaderboard = document.getElementById('leaderboard');
            const chatBox = document.getElementById('chatBox');
            const chatInputRow = document.getElementById('chatInputRow');
            const minimapCanvas = document.getElementById('minimapCanvas');

            loginOverlay.classList.remove('hidden');
            loginOverlay.classList.add('flex', 'flex-col', 'items-center', 'justify-center');
            document.body.classList.add('overlay-visible');
            stats.classList.add('hidden');
            leaderboard.classList.add('hidden');
            chatBox.classList.add('hidden');
            chatInputRow.classList.add('hidden');
            minimapCanvas.classList.add('hidden');
            
            // Reset mobile chat state when going to menu
            if (typeof chatManuallyOpened !== 'undefined') {
                chatManuallyOpened = false;
                if (chatAutoHideTimeout) {
                    clearTimeout(chatAutoHideTimeout);
                    chatAutoHideTimeout = null;
                }
            }
        });

    } catch (err) {
        console.error('Failed to initialize:', err);
        const playButton = document.getElementById('playButton');
        playButton.textContent = 'Error loading game';
        playButton.disabled = true;
    }
}

// Mobile detection and setup
function isMobileDevice() {
    return /Android|webOS|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini/i.test(navigator.userAgent) 
        || (navigator.maxTouchPoints && navigator.maxTouchPoints > 2);
}

function setupMobileControls() {
    const isMobile = isMobileDevice();
    const mobileControls = document.getElementById('mobileControls');
    const canvas = document.getElementById('gameCanvas');
    
    if (isMobile) {
        document.body.classList.add('mobile-mode');
        mobileControls.classList.add('active');
        
        // Prevent default touch behaviors on canvas
        canvas.addEventListener('touchstart', (e) => {
            e.preventDefault();
        }, { passive: false });
        
        canvas.addEventListener('touchmove', (e) => {
            e.preventDefault();
        }, { passive: false });
        
        canvas.addEventListener('touchend', (e) => {
            e.preventDefault();
        }, { passive: false });
        
        // Prevent context menu
        canvas.addEventListener('contextmenu', (e) => {
            e.preventDefault();
        });
        
        // Setup touch input handling
        setupTouchInput();
        
        // Setup double-tap detection
        setupDoubleTap();
        
        // Setup mobile button handlers
        setupMobileButtons();
        
        // Setup auto-show chat on mobile for new messages
        setupMobileChatAutoShow();
    }
}

// Auto-show chat on mobile when new messages arrive
let chatAutoHideTimeout = null;
let chatManuallyOpened = false;

function setupMobileChatAutoShow() {
    const chatBox = document.getElementById('chatBox');
    
    // Watch for new messages being added to chat
    const observer = new MutationObserver((mutations) => {
        mutations.forEach((mutation) => {
            if (mutation.addedNodes.length > 0) {
                // New chat message detected
                // Only auto-show if chat is not manually opened
                if (!chatManuallyOpened && !chatBox.classList.contains('visible')) {
                    chatBox.classList.add('visible');
                    
                    // Clear existing timeout
                    if (chatAutoHideTimeout) {
                        clearTimeout(chatAutoHideTimeout);
                    }
                    
                    // Hide after 3 seconds
                    chatAutoHideTimeout = setTimeout(() => {
                        // Only auto-hide if still not manually opened
                        if (!chatManuallyOpened) {
                            chatBox.classList.remove('visible');
                        }
                    }, 3000);
                }
            }
        });
    });
    
    // Start observing chat box for new messages
    observer.observe(chatBox, {
        childList: true,  // Watch for added/removed children
        subtree: false    // Don't watch nested elements
    });
}

// Touch state for cursor tracking
let lastTouchX = 0;
let lastTouchY = 0;
let lastPinchDistance = 0;
let currentZoomLevel = 1.0;

function setupTouchInput() {
    const canvas = document.getElementById('gameCanvas');
    
    // Prevent any native zoom behavior via touch events
    document.addEventListener('gesturestart', (e) => e.preventDefault(), { passive: false });
    document.addEventListener('gesturechange', (e) => e.preventDefault(), { passive: false });
    document.addEventListener('gestureend', (e) => e.preventDefault(), { passive: false });
    
    // Track touch position as cursor
    canvas.addEventListener('touchmove', (e) => {
        if (!gameClient) return;
        
        // Handle custom pinch-to-zoom with two fingers
        if (e.touches.length === 2) {
            e.preventDefault(); // Prevent native zoom
            
            const touch1 = e.touches[0];
            const touch2 = e.touches[1];
            
            const distance = Math.sqrt(
                Math.pow(touch2.clientX - touch1.clientX, 2) +
                Math.pow(touch2.clientY - touch1.clientY, 2)
            );
            
            if (lastPinchDistance > 0) {
                const delta = distance - lastPinchDistance;
                const zoomSensitivity = 2.5;
                
                // Simulate wheel event for zoom with smoother control
                const wheelEvent = new WheelEvent('wheel', {
                    deltaY: -delta * zoomSensitivity,
                    bubbles: true,
                    cancelable: true
                });
                canvas.dispatchEvent(wheelEvent);
            }
            
            lastPinchDistance = distance;
        }
        // Single touch - move cursor
        else if (e.touches.length === 1) {
            const touch = e.touches[0];
            lastTouchX = touch.clientX;
            lastTouchY = touch.clientY;
            
            // The game client will use mouse position for targeting
            // We simulate mousemove event to update the input state
            const mouseEvent = new MouseEvent('mousemove', {
                clientX: touch.clientX,
                clientY: touch.clientY,
                bubbles: true
            });
            canvas.dispatchEvent(mouseEvent);
            
            lastPinchDistance = 0;
        }
    }, { passive: false });
    
    // Initial touch position
    canvas.addEventListener('touchstart', (e) => {
        if (e.touches.length === 1) {
            const touch = e.touches[0];
            lastTouchX = touch.clientX;
            lastTouchY = touch.clientY;
            
            const mouseEvent = new MouseEvent('mousemove', {
                clientX: touch.clientX,
                clientY: touch.clientY,
                bubbles: true
            });
            canvas.dispatchEvent(mouseEvent);
        } else if (e.touches.length === 2) {
            e.preventDefault(); // Prevent native zoom
            // Initialize pinch distance
            const touch1 = e.touches[0];
            const touch2 = e.touches[1];
            lastPinchDistance = Math.sqrt(
                Math.pow(touch2.clientX - touch1.clientX, 2) +
                Math.pow(touch2.clientY - touch1.clientY, 2)
            );
        }
    }, { passive: false });
    
    // Reset pinch state on touch end
    canvas.addEventListener('touchend', (e) => {
        if (e.touches.length < 2) {
            lastPinchDistance = 0;
        }
    }, { passive: false });
}

// Double-tap detection for split
let lastTapTime = 0;
let lastTapX = 0;
let lastTapY = 0;
const DOUBLE_TAP_DELAY = 300; // ms
const DOUBLE_TAP_DISTANCE = 50; // pixels

function setupDoubleTap() {
    const canvas = document.getElementById('gameCanvas');
    
    canvas.addEventListener('touchend', (e) => {
        if (!gameClient) return;
        
        const now = Date.now();
        const touch = e.changedTouches[0];
        const tapX = touch.clientX;
        const tapY = touch.clientY;
        
        const timeDiff = now - lastTapTime;
        const distance = Math.sqrt(
            Math.pow(tapX - lastTapX, 2) + Math.pow(tapY - lastTapY, 2)
        );
        
        if (timeDiff < DOUBLE_TAP_DELAY && distance < DOUBLE_TAP_DISTANCE) {
            // Simulate Space keypress for split
            const keydownEvent = new KeyboardEvent('keydown', { key: ' ', code: 'Space', bubbles: true });
            document.dispatchEvent(keydownEvent);
            
            // Visual feedback
            showTapRipple(tapX, tapY);
            
            // Reset to prevent triple-tap
            lastTapTime = 0;
        } else {
            // Single tap - update for potential double-tap
            lastTapTime = now;
            lastTapX = tapX;
            lastTapY = tapY;
        }
    }, { passive: false });
}

function showTapRipple(x, y) {
    const ripple = document.createElement('div');
    ripple.className = 'tap-ripple';
    ripple.style.left = (x - 50) + 'px';
    ripple.style.top = (y - 50) + 'px';
    document.body.appendChild(ripple);
    
    setTimeout(() => {
        ripple.remove();
    }, 600);
}

function setupMobileButtons() {
    // Helper to setup button with proper touch handling (press and release)
    function setupButton(elementId, key, code, logText, isToggle = false) {
        const element = document.getElementById(elementId);
        
        element.addEventListener('touchstart', (e) => {
            e.preventDefault();
            if (isToggle) {
                // Handle toggle action (for chat)
                const chatBox = document.getElementById('chatBox');
                const wasVisible = chatBox.classList.contains('visible');
                chatBox.classList.toggle('visible');
                
                // Update manual state
                chatManuallyOpened = !wasVisible;
                
                // Clear auto-hide timeout when manually toggling
                if (chatAutoHideTimeout) {
                    clearTimeout(chatAutoHideTimeout);
                    chatAutoHideTimeout = null;
                }
            } else {
                const keydownEvent = new KeyboardEvent('keydown', { key, code, bubbles: true });
                document.dispatchEvent(keydownEvent);
            }
        });
        
        if (!isToggle) {
            element.addEventListener('touchend', (e) => {
                e.preventDefault();
                const keyupEvent = new KeyboardEvent('keyup', { key, code, bubbles: true });
                document.dispatchEvent(keyupEvent);
            });
            
            // Also handle touchcancel (when touch is interrupted)
            element.addEventListener('touchcancel', (e) => {
                e.preventDefault();
                const keyupEvent = new KeyboardEvent('keyup', { key, code, bubbles: true });
                document.dispatchEvent(keyupEvent);
            });
        }
    }
    
    // Main action buttons
    setupButton('mobileEject', 'w', 'KeyW', 'Eject');
    setupButton('mobileMenu', 'Escape', 'Escape', 'Menu');
    setupButton('mobileChat', null, null, 'Chat Toggle', true); // Toggle chat visibility
    
    // Minion control buttons
    setupButton('mobileQ', 'q', 'KeyQ');
    setupButton('mobileE', 'e', 'KeyE');
    setupButton('mobileR', 'r', 'KeyR');
    setupButton('mobileT', 't', 'KeyT');
    setupButton('mobileP', 'p', 'KeyP');
}

run();

// Initialize mobile controls after WASM loads
setTimeout(() => {
    setupMobileControls();
}, 100);

/**
 * Michi WebUI - Main Application
 * Initializes and coordinates all application modules
 */

class MichiApp {
    constructor() {
        this.initialized = false;
        this.currentRoute = null;
    }

    /**
     * Initialize the application
     */
    async init() {
        if (this.initialized) return;

        console.log('🎵 Michi WebUI initializing...');

        // Setup event listeners
        this.setupEventListeners();
        
        // Check authentication
        await this.checkAuth();
        
        // Load initial data
        await this.loadInitialData();
        
        // Start status polling
        this.startStatusPolling();
        
        this.initialized = true;
        console.log('✅ Michi WebUI ready!');
    }

    /**
     * Setup all event listeners
     */
    setupEventListeners() {
        // Navigation
        document.querySelectorAll('.nav-item').forEach(item => {
            item.addEventListener('click', (e) => {
                e.preventDefault();
                const page = item.dataset.page;
                this.navigateTo(page);
            });
        });

        // Player controls
        document.getElementById('btn-play-pause')?.addEventListener('click', () => {
            player.togglePlay();
        });

        document.getElementById('btn-prev')?.addEventListener('click', () => {
            player.previous();
        });

        document.getElementById('btn-next')?.addEventListener('click', () => {
            player.next();
        });

        document.getElementById('btn-shuffle')?.addEventListener('click', () => {
            player.toggleShuffle();
        });

        document.getElementById('btn-repeat')?.addEventListener('click', () => {
            player.toggleRepeat();
        });

        // Progress bar
        const progressBar = document.getElementById('progress-bar');
        progressBar?.addEventListener('click', (e) => {
            const rect = progressBar.getBoundingClientRect();
            const percent = (e.clientX - rect.left) / rect.width;
            player.seek(percent * player.audio.duration);
        });

        // Volume slider
        const volumeSlider = document.getElementById('volume-slider');
        volumeSlider?.addEventListener('click', (e) => {
            const rect = volumeSlider.getBoundingClientRect();
            const percent = (e.clientX - rect.left) / rect.width;
            player.setVolume(percent);
        });

        // Search
        const searchInput = document.getElementById('search-input');
        searchInput?.addEventListener('input', (e) => {
            this.handleSearch(e.target.value);
        });

        // Theme toggle
        document.getElementById('theme-toggle')?.addEventListener('click', () => {
            this.toggleTheme();
        });

        // Handle browser back/forward
        window.addEventListener('popstate', (e) => {
            if (e.state?.page) {
                this.navigateTo(e.state.page, e.state.id, false);
            }
        });

        // Keyboard shortcuts
        document.addEventListener('keydown', (e) => {
            if (e.target.tagName === 'INPUT') return;
            
            switch(e.code) {
                case 'Space':
                    e.preventDefault();
                    player.togglePlay();
                    break;
                case 'ArrowRight':
                    player.seek(player.audio.currentTime + 5);
                    break;
                case 'ArrowLeft':
                    player.seek(player.audio.currentTime - 5);
                    break;
                case 'ArrowUp':
                    e.preventDefault();
                    player.setVolume(player.volume + 0.1);
                    break;
                case 'ArrowDown':
                    e.preventDefault();
                    player.setVolume(player.volume - 0.1);
                    break;
            }
        });
    }

    /**
     * Check authentication status
     */
    async checkAuth() {
        const token = localStorage.getItem('michi_token');
        
        if (token) {
            const valid = await api.validateToken();
            if (!valid) {
                localStorage.removeItem('michi_token');
                localStorage.removeItem('michi_username');
                // Show login (for now, we'll skip actual login UI)
                console.log('Token invalid, continuing in demo mode');
            } else {
                console.log('Authenticated as', localStorage.getItem('michi_username'));
            }
        } else {
            console.log('No token, continuing in demo mode');
        }
    }

    /**
     * Load initial data
     */
    async loadInitialData() {
        try {
            // Load library stats
            const stats = await api.getStats();
            ui.updateStats(stats);
            
            // Load recently added albums
            const albums = await api.getAlbums({ limit: 8, sort: 'created_at', order: 'desc' });
            ui.renderAlbums(albums, 'recently-added');
            
            // Load most played tracks
            const tracks = await api.getTracks({ limit: 5, sort: 'play_count', order: 'desc' });
            ui.renderTracks(tracks, 'most-played');
            
        } catch (error) {
            console.error('Failed to load initial data:', error);
            ui.showToast('Failed to load library data', 'warning');
        }
    }

    /**
     * Navigate to a page
     */
    navigateTo(page, id = null, pushState = true) {
        console.log('Navigating to:', page, id);
        
        this.currentRoute = { page, id };
        ui.setActiveNav(page);
        
        if (pushState) {
            history.pushState({ page, id }, '', `#${page}${id ? `/${id}` : ''}`);
        }
        
        // Update page content based on route
        this.handleRoute(page, id);
    }

    /**
     * Handle route changes
     */
    async handleRoute(page, id) {
        const content = document.getElementById('page-content');
        
        switch(page) {
            case 'home':
                await this.loadHomePage();
                break;
            case 'albums':
                await this.loadAlbumsPage();
                break;
            case 'artists':
                await this.loadArtistsPage();
                break;
            case 'tracks':
                await this.loadTracksPage();
                break;
            case 'album':
                await this.loadAlbumDetail(id);
                break;
            default:
                content.innerHTML = `<h1 class="page-title">${page.charAt(0).toUpperCase() + page.slice(1)}</h1>`;
        }
    }

    /**
     * Load home page
     */
    async loadHomePage() {
        const content = document.getElementById('page-content');
        content.innerHTML = document.getElementById('home-page').outerHTML;
        
        // Reload stats and recent items
        await this.loadInitialData();
    }

    /**
     * Load albums page
     */
    async loadAlbumsPage() {
        const content = document.getElementById('page-content');
        content.innerHTML = `
            <h1 class="page-title">Albums</h1>
            <div class="library-grid" id="albums-grid"></div>
        `;
        
        ui.showSkeleton('albums-grid', 'album', 8);
        
        try {
            const albums = await api.getAlbums();
            ui.renderAlbums(albums, 'albums-grid');
        } catch (error) {
            ui.showToast('Failed to load albums', 'error');
        }
    }

    /**
     * Load artists page
     */
    async loadArtistsPage() {
        const content = document.getElementById('page-content');
        content.innerHTML = `
            <h1 class="page-title">Artists</h1>
            <div class="library-grid" id="artists-grid"></div>
        `;
        
        // TODO: Implement artist grid rendering
    }

    /**
     * Load tracks page
     */
    async loadTracksPage() {
        const content = document.getElementById('page-content');
        content.innerHTML = `
            <h1 class="page-title">All Tracks</h1>
            <div class="track-list" id="tracks-list"></div>
        `;
        
        ui.showSkeleton('tracks-list', 'track', 10);
        
        try {
            const tracks = await api.getTracks({ limit: 50 });
            ui.renderTracks(tracks, 'tracks-list');
        } catch (error) {
            ui.showToast('Failed to load tracks', 'error');
        }
    }

    /**
     * Load album detail page
     */
    async loadAlbumDetail(id) {
        const content = document.getElementById('page-content');
        
        try {
            const album = await api.getAlbum(id);
            const tracks = await api.getAlbumTracks(id);
            
            content.innerHTML = `
                <div class="hero-section gradient-hero">
                    <div class="hero-gradient"></div>
                    <div class="hero-content">
                        <img src="${api.getCoverUrl(id, 300)}" alt="${album.title}" class="hero-cover">
                        <div class="hero-info">
                            <div class="badge badge-info">Album</div>
                            <h1 class="hero-title">${album.title}</h1>
                            <p class="hero-subtitle">${album.artist} ${album.year ? `• ${album.year}` : ''}</p>
                            <div class="hero-actions">
                                <button class="btn btn-primary" onclick="app.playAlbum('${id}')">
                                    <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor" style="margin-right: 8px;">
                                        <polygon points="5 3 19 12 5 21 5 3"/>
                                    </svg>
                                    Play Album
                                </button>
                                <button class="btn btn-secondary">
                                    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                        <path d="M20.84 4.61a5.5 5.5 0 00-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 00-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 000-7.78z"/>
                                    </svg>
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
                <div class="track-list" style="margin-top: var(--spacing-6);" id="album-tracks"></div>
            `;
            
            ui.renderTracks(tracks, 'album-tracks');
        } catch (error) {
            ui.showToast('Failed to load album', 'error');
        }
    }

    /**
     * Play an album
     */
    async playAlbum(id) {
        try {
            const tracks = await api.getAlbumTracks(id);
            if (tracks.length > 0) {
                await player.play(tracks[0], tracks);
            }
        } catch (error) {
            ui.showToast('Failed to play album', 'error');
        }
    }

    /**
     * Play a track
     */
    async playTrack(id, tracks = []) {
        try {
            const track = await api.getTrack(id);
            await player.play(track, tracks);
        } catch (error) {
            ui.showToast('Failed to play track', 'error');
        }
    }

    /**
     * Handle search
     */
    async handleSearch(query) {
        if (!query || query.length < 2) return;
        
        // Debounce search
        clearTimeout(this.searchTimeout);
        this.searchTimeout = setTimeout(async () => {
            try {
                const results = await api.search(query);
                console.log('Search results:', results);
                // TODO: Show search results dropdown
            } catch (error) {
                console.error('Search failed:', error);
            }
        }, 300);
    }

    /**
     * Toggle theme
     */
    toggleTheme() {
        // TODO: Implement light/dark theme toggle
        ui.showToast('Theme toggle coming soon!', 'info');
    }

    /**
     * Start server status polling
     */
    startStatusPolling() {
        setInterval(async () => {
            try {
                const status = await api.getServerStatus();
                ui.updateServerStatus(status.online !== false);
            } catch (error) {
                ui.updateServerStatus(false);
            }
        }, 30000); // Poll every 30 seconds
    }
}

// Create singleton instance
const app = new MichiApp();

// Initialize on DOM ready
if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => app.init());
} else {
    app.init();
}

export default app;

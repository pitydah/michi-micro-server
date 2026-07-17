/**
 * Michi WebUI - UI Manager
 * Handles all UI updates, rendering, and user interactions
 */

class UIManager {
    constructor() {
        this.currentPage = 'home';
        this.toasts = [];
    }

    /**
     * Format time in MM:SS or HH:MM:SS
     */
    formatTime(seconds) {
        if (!seconds || isNaN(seconds)) return '0:00';
        
        const hrs = Math.floor(seconds / 3600);
        const mins = Math.floor((seconds % 3600) / 60);
        const secs = Math.floor(seconds % 60);
        
        if (hrs > 0) {
            return `${hrs}:${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`;
        }
        return `${mins}:${secs.toString().padStart(2, '0')}`;
    }

    /**
     * Format number with K/M suffixes
     */
    formatNumber(num) {
        if (num >= 1000000) {
            return (num / 1000000).toFixed(1) + 'M';
        }
        if (num >= 1000) {
            return (num / 1000).toFixed(1) + 'K';
        }
        return num.toString();
    }

    /**
     * Update Now Playing bar
     */
    updateNowPlaying(track) {
        const coverEl = document.getElementById('np-cover');
        const titleEl = document.getElementById('np-title');
        const artistEl = document.getElementById('np-artist');
        
        if (track) {
            coverEl.src = api.getCoverUrl(track.album_id || track.id, 64);
            titleEl.textContent = track.title || 'Unknown Title';
            artistEl.textContent = track.artist || 'Unknown Artist';
        } else {
            coverEl.src = '/api/v1/cover?size=64';
            titleEl.textContent = 'Not Playing';
            artistEl.textContent = 'Select a track';
        }
    }

    /**
     * Update play/pause button
     */
    updatePlayButton(isPlaying) {
        const playIcon = document.getElementById('play-icon');
        const pauseIcon = document.getElementById('pause-icon');
        
        if (isPlaying) {
            playIcon.style.display = 'none';
            pauseIcon.style.display = 'block';
        } else {
            playIcon.style.display = 'block';
            pauseIcon.style.display = 'none';
        }
    }

    /**
     * Update progress bar
     */
    updateProgress(current, total) {
        const fill = document.getElementById('progress-fill');
        const currentEl = document.getElementById('progress-current');
        const totalEl = document.getElementById('progress-total');
        
        const percent = total > 0 ? (current / total) * 100 : 0;
        fill.style.width = `${percent}%`;
        
        currentEl.textContent = this.formatTime(current);
        totalEl.textContent = this.formatTime(total);
    }

    /**
     * Update duration display
     */
    updateDuration(duration) {
        const totalEl = document.getElementById('progress-total');
        totalEl.textContent = this.formatTime(duration);
    }

    /**
     * Update volume display
     */
    updateVolumeDisplay(volume) {
        const fill = document.getElementById('volume-fill');
        fill.style.width = `${volume * 100}%`;
    }

    /**
     * Update shuffle state
     */
    updateShuffleState(shuffle) {
        const btn = document.getElementById('btn-shuffle');
        if (shuffle) {
            btn.style.color = 'var(--accent-primary)';
        } else {
            btn.style.color = 'var(--text-normal)';
        }
    }

    /**
     * Update repeat state
     */
    updateRepeatState(repeat) {
        const btn = document.getElementById('btn-repeat');
        
        if (repeat === 'off') {
            btn.style.color = 'var(--text-normal)';
        } else if (repeat === 'all') {
            btn.style.color = 'var(--accent-primary)';
        } else if (repeat === 'one') {
            btn.style.color = 'var(--accent-secondary)';
        }
    }

    /**
     * Show toast notification
     */
    showToast(message, type = 'info') {
        const container = document.querySelector('.toast-container') || this.createToastContainer();
        
        const toast = document.createElement('div');
        toast.className = `toast toast-${type}`;
        toast.innerHTML = `
            <svg class="toast-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                ${type === 'success' ? '<path d="M22 11.08V12a10 10 0 11-5.93-9.14"/><polyline points="22 4 12 14.01 9 11.01"/>' : ''}
                ${type === 'error' ? '<circle cx="12" cy="12" r="10"/><line x1="15" y1="9" x2="9" y2="15"/><line x1="9" y1="9" x2="15" y2="15"/>' : ''}
                ${type === 'warning' ? '<path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/>' : ''}
                ${type === 'info' ? '<circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/>' : ''}
            </svg>
            <span class="toast-message">${message}</span>
        `;
        
        container.appendChild(toast);
        
        // Auto-remove after 4 seconds
        setTimeout(() => {
            toast.style.animation = 'slide-in 0.3s reverse';
            setTimeout(() => toast.remove(), 300);
        }, 4000);
    }

    createToastContainer() {
        const container = document.createElement('div');
        container.className = 'toast-container';
        document.body.appendChild(container);
        return container;
    }

    /**
     * Render album cards
     */
    renderAlbums(albums, containerId) {
        const container = document.getElementById(containerId);
        if (!container || !albums) return;
        
        container.innerHTML = albums.map(album => `
            <div class="album-card hover-lift" data-album-id="${album.id}">
                <div class="album-cover">
                    <img src="${api.getCoverUrl(album.id, 300)}" alt="${album.title}" loading="lazy">
                    <div class="album-overlay">
                        <button class="play-overlay-btn" onclick="app.playAlbum('${album.id}')">
                            <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor">
                                <polygon points="5 3 19 12 5 21 5 3"/>
                            </svg>
                        </button>
                    </div>
                </div>
                <div class="album-info">
                    <div class="album-title">${album.title}</div>
                    <div class="album-artist">${album.artist}</div>
                    <div class="album-meta">
                        ${album.year ? `<span class="badge badge-info">${album.year}</span>` : ''}
                        ${album.quality ? `<span class="quality-indicator quality-${album.quality.toLowerCase()}">${album.quality}</span>` : ''}
                    </div>
                </div>
            </div>
        `).join('');
        
        // Add click handlers
        container.querySelectorAll('.album-card').forEach(card => {
            card.addEventListener('click', (e) => {
                if (!e.target.closest('.play-overlay-btn')) {
                    app.navigateTo('album', card.dataset.albumId);
                }
            });
        });
    }

    /**
     * Render track list
     */
    renderTracks(tracks, containerId) {
        const container = document.getElementById(containerId);
        if (!container || !tracks) return;
        
        container.innerHTML = tracks.map((track, index) => `
            <div class="track-row" data-track-id="${track.id}">
                <div class="track-number">${index + 1}</div>
                <div class="track-title-cell">
                    ${track.cover_url ? `<img src="${track.cover_url}" class="cover-art" style="width: 40px; height: 40px;">` : ''}
                    <div>
                        <div class="track-title">${track.title}</div>
                        ${track.artist ? `<div class="track-artist">${track.artist}</div>` : ''}
                    </div>
                </div>
                <div class="track-album">${track.album || ''}</div>
                <div class="track-artist">${track.artist || ''}</div>
                <div class="track-duration">${this.formatTime(track.duration)}</div>
                <div class="track-actions">
                    <button class="track-action-btn" title="Play">
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                            <polygon points="5 3 19 12 5 21 5 3"/>
                        </svg>
                    </button>
                    <button class="track-action-btn" title="Add to playlist">
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                            <path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01"/>
                        </svg>
                    </button>
                    <button class="track-action-btn" title="More">
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                            <circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/><circle cx="5" cy="12" r="1"/>
                        </svg>
                    </button>
                </div>
            </div>
        `).join('');
        
        // Add click handlers
        container.querySelectorAll('.track-row').forEach(row => {
            row.addEventListener('click', (e) => {
                if (!e.target.closest('.track-actions')) {
                    app.playTrack(row.dataset.trackId, tracks);
                }
            });
        });
    }

    /**
     * Update stats display
     */
    updateStats(stats) {
        if (stats.albums !== undefined) {
            document.getElementById('stat-albums').textContent = this.formatNumber(stats.albums);
        }
        if (stats.artists !== undefined) {
            document.getElementById('stat-artists').textContent = this.formatNumber(stats.artists);
        }
        if (stats.tracks !== undefined) {
            document.getElementById('stat-tracks').textContent = this.formatNumber(stats.tracks);
        }
        if (stats.genres !== undefined) {
            document.getElementById('stat-genres').textContent = this.formatNumber(stats.genres);
        }
    }

    /**
     * Update server status
     */
    updateServerStatus(online) {
        const indicator = document.getElementById('server-status');
        const text = document.getElementById('status-text');
        
        if (online) {
            indicator.className = 'status-indicator status-online';
            text.textContent = 'Connected';
        } else {
            indicator.className = 'status-indicator status-offline';
            text.textContent = 'Disconnected';
        }
    }

    /**
     * Set active nav item
     */
    setActiveNav(page) {
        document.querySelectorAll('.nav-item').forEach(item => {
            item.classList.toggle('active', item.dataset.page === page);
        });
    }

    /**
     * Show loading skeleton
     */
    showSkeleton(containerId, type = 'album', count = 4) {
        const container = document.getElementById(containerId);
        if (!container) return;
        
        if (type === 'album') {
            container.innerHTML = Array(count).fill('<div class="album-card skeleton"></div>').join('');
        } else if (type === 'track') {
            container.innerHTML = Array(count).fill('<div class="track-row skeleton"></div>').join('');
        }
    }

    /**
     * Clear loading skeleton
     */
    clearSkeleton(containerId) {
        const container = document.getElementById(containerId);
        if (container) {
            container.querySelectorAll('.skeleton').forEach(el => el.remove());
        }
    }
}

// Create singleton instance
const ui = new UIManager();

export default ui;

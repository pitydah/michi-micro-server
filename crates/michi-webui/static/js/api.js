/**
 * Michi WebUI - API Client
 * Handles all communication with Michi Micro Server API v1
 */

const API_BASE = '/api/v1';

class MichiAPI {
    constructor() {
        this.baseURL = API_BASE;
        this.token = null;
        this.username = null;
    }

    /**
     * Set authentication token
     */
    setAuth(token, username) {
        this.token = token;
        this.username = username;
    }

    /**
     * Get authentication headers
     */
    getHeaders() {
        const headers = {
            'Content-Type': 'application/json',
        };
        
        if (this.token) {
            headers['X-API-Token'] = this.token;
        }
        
        return headers;
    }

    /**
     * Generic request handler
     */
    async request(endpoint, options = {}) {
        const url = `${this.baseURL}${endpoint}`;
        const config = {
            ...options,
            headers: {
                ...this.getHeaders(),
                ...options.headers,
            },
        };

        try {
            const response = await fetch(url, config);
            
            if (!response.ok) {
                const error = await response.json().catch(() => ({
                    message: 'Request failed',
                }));
                throw new Error(error.message || `HTTP ${response.status}`);
            }
            
            // Handle empty responses
            const contentType = response.headers.get('content-type');
            if (contentType && contentType.includes('application/json')) {
                return await response.json();
            }
            
            return await response.blob();
        } catch (error) {
            console.error('API Request failed:', error);
            throw error;
        }
    }

    /**
     * Authentication
     */
    async login(username, password) {
        const response = await this.request('/auth/login', {
            method: 'POST',
            body: JSON.stringify({ username, password }),
        });
        
        if (response.token) {
            this.setAuth(response.token, username);
            localStorage.setItem('michi_token', response.token);
            localStorage.setItem('michi_username', username);
        }
        
        return response;
    }

    async logout() {
        await this.request('/auth/logout', { method: 'POST' });
        this.token = null;
        this.username = null;
        localStorage.removeItem('michi_token');
        localStorage.removeItem('michi_username');
    }

    async validateToken() {
        try {
            await this.request('/auth/validate');
            return true;
        } catch {
            return false;
        }
    }

    /**
     * Library Stats
     */
    async getStats() {
        return this.request('/library/stats');
    }

    /**
     * Albums
     */
    async getAlbums(params = {}) {
        const query = new URLSearchParams(params).toString();
        return this.request(`/albums${query ? `?${query}` : ''}`);
    }

    async getAlbum(id) {
        return this.request(`/albums/${id}`);
    }

    async getAlbumTracks(id) {
        return this.request(`/albums/${id}/tracks`);
    }

    /**
     * Artists
     */
    async getArtists(params = {}) {
        const query = new URLSearchParams(params).toString();
        return this.request(`/artists${query ? `?${query}` : ''}`);
    }

    async getArtist(id) {
        return this.request(`/artists/${id}`);
    }

    async getArtistAlbums(id) {
        return this.request(`/artists/${id}/albums`);
    }

    /**
     * Tracks
     */
    async getTracks(params = {}) {
        const query = new URLSearchParams(params).toString();
        return this.request(`/tracks${query ? `?${query}` : ''}`);
    }

    async getTrack(id) {
        return this.request(`/tracks/${id}`);
    }

    async getTrackStream(id) {
        return `${this.baseURL}/tracks/${id}/stream`;
    }

    /**
     * Genres
     */
    async getGenres() {
        return this.request('/genres');
    }

    async getGenre(name) {
        return this.request(`/genres/${encodeURIComponent(name)}`);
    }

    /**
     * Playlists
     */
    async getPlaylists() {
        return this.request('/playlists');
    }

    async getPlaylist(id) {
        return this.request(`/playlists/${id}`);
    }

    async createPlaylist(name, description = '') {
        return this.request('/playlists', {
            method: 'POST',
            body: JSON.stringify({ name, description }),
        });
    }

    async addToPlaylist(playlistId, trackIds) {
        return this.request(`/playlists/${playlistId}/tracks`, {
            method: 'POST',
            body: JSON.stringify({ track_ids: trackIds }),
        });
    }

    async removeFromPlaylist(playlistId, trackId) {
        return this.request(`/playlists/${playlistId}/tracks/${trackId}`, {
            method: 'DELETE',
        });
    }

    /**
     * Search
     */
    async search(query, types = ['tracks', 'albums', 'artists']) {
        const params = new URLSearchParams({ q: query, type: types.join(',') });
        return this.request(`/search?${params}`);
    }

    /**
     * Playback
     */
    async scrobble(trackId, timestamp = Date.now()) {
        return this.request('/playback/scrobble', {
            method: 'POST',
            body: JSON.stringify({ track_id: trackId, timestamp }),
        });
    }

    async setNowPlaying(trackId) {
        return this.request('/playback/now-playing', {
            method: 'POST',
            body: JSON.stringify({ track_id: trackId }),
        });
    }

    /**
     * User Preferences
     */
    async getPreferences() {
        return this.request('/user/preferences');
    }

    async updatePreferences(prefs) {
        return this.request('/user/preferences', {
            method: 'PUT',
            body: JSON.stringify(prefs),
        });
    }

    /**
     * Cover Art
     */
    getCoverUrl(id, size = 300) {
        return `${this.baseURL}/cover/${id}?size=${size}`;
    }

    /**
     * Server Info
     */
    async getServerInfo() {
        return this.request('/server/info');
    }

    async getServerStatus() {
        return this.request('/server/status');
    }
}

// Create singleton instance
const api = new MichiAPI();

// Auto-load credentials from localStorage
const storedToken = localStorage.getItem('michi_token');
const storedUsername = localStorage.getItem('michi_username');
if (storedToken && storedUsername) {
    api.setAuth(storedToken, storedUsername);
}

export default api;

/**
 * Michi WebUI - Audio Player
 * Handles audio playback, queue management, and player state
 */

class AudioPlayer {
    constructor() {
        this.audio = document.getElementById('audio-player');
        this.isPlaying = false;
        this.currentTrack = null;
        this.queue = [];
        this.queueIndex = 0;
        this.shuffle = false;
        this.repeat = 'off'; // 'off', 'all', 'one'
        this.volume = 0.7;
        
        this.init();
    }

    init() {
        // Audio events
        this.audio.addEventListener('play', () => this.onPlay());
        this.audio.addEventListener('pause', () => this.onPause());
        this.audio.addEventListener('ended', () => this.onEnded());
        this.audio.addEventListener('timeupdate', () => this.onTimeUpdate());
        this.audio.addEventListener('loadedmetadata', () => this.onLoadedMetadata());
        this.audio.addEventListener('error', (e) => this.onError(e));
        
        // Load saved volume
        const savedVolume = localStorage.getItem('michi_volume');
        if (savedVolume !== null) {
            this.setVolume(parseFloat(savedVolume));
        }
    }

    /**
     * Play a track
     */
    async play(track, tracks = []) {
        try {
            this.currentTrack = track;
            
            // Set up queue if provided
            if (tracks.length > 0) {
                this.queue = tracks;
                this.queueIndex = tracks.findIndex(t => t.id === track.id);
            }
            
            // Get stream URL
            const streamUrl = api.getTrackStream(track.id);
            this.audio.src = streamUrl;
            
            // Update UI
            ui.updateNowPlaying(track);
            
            // Play
            await this.audio.play();
            this.isPlaying = true;
            
            // Scrobble
            await api.setNowPlaying(track.id);
            
        } catch (error) {
            console.error('Failed to play track:', error);
            ui.showToast('Failed to play track', 'error');
        }
    }

    /**
     * Pause playback
     */
    pause() {
        this.audio.pause();
        this.isPlaying = false;
    }

    /**
     * Toggle play/pause
     */
    togglePlay() {
        if (this.isPlaying) {
            this.pause();
        } else {
            this.audio.play().catch(console.error);
        }
    }

    /**
     * Play next track
     */
    async next() {
        if (this.queue.length === 0) return;
        
        if (this.shuffle) {
            this.queueIndex = Math.floor(Math.random() * this.queue.length);
        } else {
            this.queueIndex = (this.queueIndex + 1) % this.queue.length;
            
            // Handle end of queue
            if (this.queueIndex === 0 && this.repeat === 'off') {
                this.queueIndex = this.queue.length - 1;
            }
        }
        
        await this.play(this.queue[this.queueIndex]);
    }

    /**
     * Play previous track
     */
    async previous() {
        if (this.queue.length === 0) return;
        
        // If more than 3 seconds in, restart current track
        if (this.audio.currentTime > 3) {
            this.audio.currentTime = 0;
            return;
        }
        
        this.queueIndex = (this.queueIndex - 1 + this.queue.length) % this.queue.length;
        await this.play(this.queue[this.queueIndex]);
    }

    /**
     * Seek to position
     */
    seek(position) {
        this.audio.currentTime = position;
    }

    /**
     * Set volume
     */
    setVolume(level) {
        this.volume = Math.max(0, Math.min(1, level));
        this.audio.volume = this.volume;
        localStorage.setItem('michi_volume', this.volume.toString());
        ui.updateVolumeDisplay(this.volume);
    }

    /**
     * Toggle shuffle
     */
    toggleShuffle() {
        this.shuffle = !this.shuffle;
        ui.updateShuffleState(this.shuffle);
    }

    /**
     * Cycle repeat mode
     */
    toggleRepeat() {
        const modes = ['off', 'all', 'one'];
        const currentIndex = modes.indexOf(this.repeat);
        this.repeat = modes[(currentIndex + 1) % modes.length];
        ui.updateRepeatState(this.repeat);
    }

    /**
     * Event Handlers
     */
    onPlay() {
        this.isPlaying = true;
        ui.updatePlayButton(true);
    }

    onPause() {
        this.isPlaying = false;
        ui.updatePlayButton(false);
    }

    onEnded() {
        if (this.repeat === 'one') {
            this.audio.currentTime = 0;
            this.audio.play();
        } else {
            this.next();
        }
    }

    onTimeUpdate() {
        ui.updateProgress(
            this.audio.currentTime,
            this.audio.duration || 0
        );
    }

    onLoadedMetadata() {
        ui.updateDuration(this.audio.duration);
    }

    onError(e) {
        console.error('Audio error:', e);
        ui.showToast('Playback error', 'error');
    }

    /**
     * Get current state
     */
    getState() {
        return {
            isPlaying: this.isPlaying,
            currentTrack: this.currentTrack,
            queue: this.queue,
            queueIndex: this.queueIndex,
            shuffle: this.shuffle,
            repeat: this.repeat,
            volume: this.volume,
            currentTime: this.audio.currentTime,
            duration: this.audio.duration,
        };
    }
}

// Create singleton instance
const player = new AudioPlayer();

export default player;

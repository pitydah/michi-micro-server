# Home Assistant Integration (Future)

Michi Micro Server will integrate with Home Assistant via MQTT Discovery, allowing control of music playback from the Home Assistant dashboard.

## Planned Integration

### MQTT Discovery Topics

Michi Micro Server will publish discovery configurations under:
```
homeassistant/media_player/michi/config
homeassistant/sensor/michi_playing/config
homeassistant/switch/michi_random/config
```

### Controls

| Feature | Type | Description |
|---------|------|-------------|
| Play/Pause | media_player | Control playback |
| Next/Previous | media_player | Track navigation |
| Volume | media_player | Volume control |
| Source | media_player | Select playlist/album |
| Now Playing | sensor | Current track info |
| Shuffle | switch | Toggle shuffle |
| Repeat | switch | Toggle repeat |

### MQTT Topics

```
michi/playback/command
michi/playback/state
michi/player/volume/set
michi/player/volume/state
michi/player/next
michi/player/previous
michi/library/scan
```

## Technology

MQTT communication will use the `rumqttc` crate for async MQTT client operations.

## Configuration

Home Assistant integration will be configured via environment variables:

```
MICHI_MQTT_HOST=core-mosquitto
MICHI_MQTT_PORT=1883
MICHI_MQTT_USER=michi
MICHI_MQTT_PASS=secret
MICHI_HA_ENABLED=true
```

## Status

This integration is planned for **Phase 5** of the roadmap.

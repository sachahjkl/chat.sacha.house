[English](README.md) | [Français](README.fr.md)

# chat.sacha.house

Salon de discussion global unique sans authentification. Choisissez un nom d'utilisateur et discutez en temps réel.

## Technologies

- **Backend** : Rust + warp + SQLite
- **Frontend** : Svelte 5 + Bun + Vite
- **Temps réel** : Server-Sent Events (SSE)

## Compilation

```bash
nix build
```

## Exécution

```bash
cargo run
```

Le serveur démarre sur `http://127.0.0.1:3030`. Configurez l'adresse avec les variables d'environnement `BIND_HOST` et `BIND_PORT`.

Variables d'environnement :
- `BIND_HOST` : adresse d'écoute du serveur (valeur par défaut : `127.0.0.1`)
- `BIND_PORT` : port du serveur (valeur par défaut : `3030`)
- `RATE_LIMIT_SECS` : fenêtre de limitation en secondes pour l'envoi de messages (valeur par défaut : `1`)

## Conteneur

```bash
nix build .#dockerImage
podman load < result
podman run --rm -p 3030:3030 chat-sacha-house:0.1.0
```

## Fonctionnalités

- Réservation du nom d'utilisateur avec un cookie de session
- Transmission des messages en temps réel avec SSE
- Historique illimité des messages dans SQLite
- Limitation à un message par seconde et par session
- Libération automatique du nom d'utilisateur après 60 secondes d'inactivité

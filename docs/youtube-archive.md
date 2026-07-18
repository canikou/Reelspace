# YouTube archive authentication

Reelspace uses Google's installed-desktop OAuth flow. A user clicks **Connect YouTube**, signs in through their normal browser, approves the requested YouTube permissions, and returns automatically through a temporary loopback callback. Reelspace uses PKCE for the authorization exchange and keeps refresh/access tokens in Windows Credential Manager.

Users never enter a client ID or client secret. The public desktop client ID identifies Reelspace to Google and is bundled into official builds. Like all installed-app identifiers, it can be extracted from the distributed executable and is not treated as a secret. Reelspace does not bundle or use the Google-generated desktop client secret.

## Maintainer setup

The Google Cloud project behind an official build must have:

1. YouTube Data API v3 enabled.
2. Google Auth Platform branding configured for Reelspace.
3. An External audience. While publishing status is **Testing**, every permitted Google account must be listed under **Audience → Test users**.
4. A Desktop app OAuth client.
5. The `youtube.upload` and `youtube.readonly` scopes declared on the Data Access page.

The official Reelspace client ID is the default in `src-tauri/src/youtube.rs`. A fork or private build can override it without editing source:

```powershell
$env:REELSPACE_YOUTUBE_CLIENT_ID = "your-client-id.apps.googleusercontent.com"
pnpm tauri:build
```

Do not put OAuth access tokens, refresh tokens, account passwords, or unrelated API credentials in source control. No Reelspace server or web redirect URI is required for this flow.

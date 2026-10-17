# Models

Sailry uses your model providers and accounts. Configure the host where you want
to run tasks; a remote host does not automatically receive your local credentials.

## Add a provider

1. Select the target host, then open **Settings → AI providers**.
2. Choose **Add provider** and the provider type.
3. For an API provider, enter its **API endpoint** and **API Key**. Use the
   provider's documented endpoint and matching protocol. OpenAI-compatible
   providers can use **Responses** or **Chat**; Anthropic-compatible and
   Gemini-compatible options are separate provider types.
4. Configure **Models**, then **Save**. Use **Update models** to discover models
   when the provider supports it, or **New model** to enter a **Model ID**.
   Choose **Set as default** for the model you want selected by default.

For account sign-in, choose **ChatGPT OAuth**, **ChatGPT SIWC** or **Claude OAuth**.
Save the provider first, then use **Connect account** and follow its browser or
device-code flow. If asked for an authorization code or browser redirect URL,
paste the result into that sign-in dialog. Access depends on your account and plan.

## Choose a conversation model

Use the model selector in the conversation composer. A model change applies to
subsequent tasks in that conversation; tasks already sent or queued retain their
captured configuration. Existing sessions keep their model selection when resumed.

Provider settings opened from a conversation can belong to that model's original
configuration host. Check the selected host before editing shared credentials.

## If a model is unavailable

- Check the endpoint, protocol and exact model ID against your provider's settings.
- For rejected API credentials, update the key on the execution host.
- For an expired account connection, use **Sign in again**.
- If sign-in succeeds but no models appear, retry **Update models** or add the
  supported model ID manually. A saved configuration does not establish account access.

Credentials are stored as plaintext in the execution host's data profile.
Do not share the profile or include keys in screenshots and issue reports.

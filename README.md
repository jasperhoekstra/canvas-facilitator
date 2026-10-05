# Canvas Facilitator

Nederlandstalige desktopapp (Windows/macOS) die in maximaal 15 minuten een AI-idee via een gesproken gesprek uitwerkt tot een vijf-stappencanvas: **KIES → MEET → BEGRENS → REALISEER → VERANKER**. Eigen OpenAI-key (BYOK), lokaal versleutelde opslag, harde native deadline en kostenbewaking.

- Specificatie: [`docs/PRD-v1.0.md`](docs/PRD-v1.0.md)
- Architectuur: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Acceptatiestatus: [`docs/ACCEPTANCE.md`](docs/ACCEPTANCE.md) — nog geen v1.0-release
- Release: [`docs/RELEASE.md`](docs/RELEASE.md) · Repository: [`docs/REPOSITORY.md`](docs/REPOSITORY.md)

## Ontwikkelen

Vereist: Node 22+, Rust stable, en op Windows de MSVC Build Tools en [Strawberry Perl](https://strawberryperl.com/) (voor de gevendorde OpenSSL van SQLCipher; zet `PERL=C:\Strawberry\perl\bin\perl.exe`). Houd het pad van de checkout kort: Windows' 260-tekenlimiet kan de OpenSSL-build breken.

```bash
cd apps/desktop
npm ci
npx tauri dev                         # app met hot reload
npm test && npm run build             # frontend
cd src-tauri && cargo test            # Rust-kern
npm run screenshots                   # AC-UI screenshots (mock-IPC, headless Edge/Chrome)
OPENAI_API_KEY=sk-... cargo run --example fac_eval   # AC-FAC scenario's (kost geld)
```

`.env.example` is alleen voor die evaluatierun; eindgebruikers voeren hun key in de app in.

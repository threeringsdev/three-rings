import path from "node:path";

// storageState written by auth.setup.ts (the login fixture). Authed tests
// opt in with `test.use({ storageState: AUTH_STATE })`.
export const AUTH_STATE = path.join(__dirname, "../playwright/.auth/user.json");

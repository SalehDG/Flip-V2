import { invoke } from "@tauri-apps/api/core";
import type { GmailHistoryEmail } from "../types/emailHistory";


export function getGmailHistory(accountId?: number): Promise<GmailHistoryEmail[]> {
  return invoke<GmailHistoryEmail[]>("get_gmail_history", { accountId: accountId ?? null });
}


export function exportGmailMessageDiagnostic(accountId: number, messageId: string): Promise<string> {
  return invoke<string>("export_gmail_message_diagnostic", { accountId, messageId });
}

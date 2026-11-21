export interface GmailHistoryAttachment {
  id: number;
  filename: string | null;
  declaredMimeType: string | null;
  detectedFormat: string;
  fileSize: number;
  sha256: string;
  localPath: string;
  fileExists: boolean;
  createdAt: string;
}

export interface GmailHistoryEmail {
  accountId: number;
  accountEmail: string;
  messageId: string;
  threadId: string;
  internalDate: string | null;
  sender: string | null;
  subject: string | null;
  snippet: string | null;
  retrievedAt: string;
  attachments: GmailHistoryAttachment[];
}

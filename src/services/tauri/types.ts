export interface ExtractionSummary {
  processed: number; completed: number; needsReview: number; duplicateReview: number; failed: number; skipped: number;
}
export interface ExtractionStatusSummary {
  detected: number; parsed: number; ocrProcessing: number; completed: number; needsReview: number; duplicateReview: number; failed: number;
}
export interface ExtractionTransaction {
  id: number; gmailAccountId: number; accountEmail: string; gmailMessageId: string;
  flipTransactionId: string | null; transactionDate: string | null; recipientName: string | null;
  amount: number | null; transferNote: string | null; ocrRawResult: string | null; status: string;
  reviewReason: string | null; lastError: string | null; pngPath: string | null; attemptCount: number;
  lastAttemptAt: string | null; createdAt: string; updatedAt: string;
  firstCopiedAt: string | null; lastCopiedAt: string | null;
}
export interface ExtractionProgressEvent { transactionId: number; status: string; message: string | null; }

import { invoke } from '@tauri-apps/api/core';
import type { ExtractionStatusSummary, ExtractionSummary, ExtractionTransaction, RetentionSettings, RetentionPreview, RetentionRunSummary, ProcessingLogRecord } from './types';

export const tauriBridge = {
  getExtractionSummary: () => invoke<ExtractionStatusSummary>('get_extraction_status_summary'),
  getTransactions: (args: { limit?: number; offset?: number; status?: string | null } = {}) =>
    invoke<ExtractionTransaction[]>('get_extraction_transactions', { limit: args.limit ?? 100, offset: args.offset ?? 0, status: args.status ?? null }),
  runExtraction: () => invoke<ExtractionSummary>('run_transaction_extraction'),
  retryExtraction: (transactionId: number) => invoke<string>('retry_transaction_extraction_command', { transactionId }),
  correctTransferNote: (transactionId: number, transferNote: string) =>
    invoke<string>('correct_transaction_transfer_note', { transactionId, transferNote }),
  saveTransactionCorrection: (args: { transactionId: number; transactionDate: string; recipientName: string; flipTransactionId: string; amount: number; transferNote: string }) =>
    invoke<string>('save_transaction_correction', args),
  getProcessingLogs: (limit = 200) => invoke<ProcessingLogRecord[]>('get_processing_logs', { limit }),
  markTransactionsCopied: (transactionIds: number[]) =>
    invoke<number>('mark_transactions_copied', { transactionIds }),
  getRetentionSettings: () => invoke<RetentionSettings>('get_retention_settings'),
  setRetentionSettings: (retentionDays: number) => invoke<RetentionSettings>('set_retention_settings', { retentionDays }),
  previewRetentionCleanup: () => invoke<RetentionPreview>('preview_retention_cleanup'),
  runRetentionCleanup: () => invoke<RetentionRunSummary>('run_retention_cleanup'),
};

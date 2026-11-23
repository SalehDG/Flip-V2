import { tauriBridge } from '../../../services/tauri/bridge';
export const transactionService = {
  list: tauriBridge.getTransactions,
  retry: tauriBridge.retryExtraction,
  correctNote: tauriBridge.correctTransferNote,
  saveCorrection: tauriBridge.saveTransactionCorrection,
};

import type { ProfileTestError } from "@/ipc/bindings";

export type ProfileTestErrorDisplay = {
  message: string;
  details: string | null;
};

export function formatProfileTestError(error: ProfileTestError): ProfileTestErrorDisplay {
  const message = error.message.trim();
  const rawDetails = error.details;
  if (rawDetails == null || rawDetails.trim().length === 0) {
    return { message, details: null };
  }

  let details = rawDetails;
  try {
    details = JSON.stringify(JSON.parse(rawDetails), null, 2);
  } catch {
    // Non-JSON Provider responses must remain byte-for-byte readable in the UI.
  }

  return {
    message: rawDetails.trim() === message ? "" : message,
    details,
  };
}

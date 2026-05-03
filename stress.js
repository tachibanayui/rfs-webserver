import http from "k6/http";
import { textSummary } from "https://jslib.k6.io/k6-summary/0.0.2/index.js";

export const options = {
    scenarios: {
        flood_test: {
            executor: "constant-vus",
            vus: 100,
            duration: "30s",
        },
    },
};

export default function () {
    // Paste the left most leaf in the vfs tree here, using seed 12345
    http.get(
        "http://localhost:3000/billing/2026/04/batch/invoices-a9a084b3/pending/04-11cfce56/billing/orders/archive/",
    );
}

export function handleSummary(data) {
    return {
        stdout: textSummary(data, { indent: " ", enableColors: true }),
        "stress-result.md": textSummary(data, {
            indent: " ",
            enableColors: false,
        }),
    };
}

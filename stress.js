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
        "http://localhost:3000/archive-137accdc/2024-377fd2c5/06-2df993fe/06-5232d311/05-26865c8f/04-5f5652ac/2024/04-baebfbb8/05-2038586b/05-f5acaab5/",
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

import note from "./note.txt";

export default {
  fetch() {
    return new Response(note);
  },
};

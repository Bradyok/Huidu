// DecompileExport.java — dump decompiled C for every function to one .c file.
// Headless usage: -postScript DecompileExport.java <output.c>
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import ghidra.program.model.listing.FunctionManager;
import ghidra.util.task.ConsoleTaskMonitor;
import java.io.FileWriter;
import java.io.PrintWriter;

public class DecompileExport extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String outpath = args.length > 0 ? args[0] : "decomp.c";

        DecompInterface decomp = new DecompInterface();
        decomp.openProgram(currentProgram);
        ConsoleTaskMonitor monitor = new ConsoleTaskMonitor();

        FunctionManager fm = currentProgram.getFunctionManager();
        int total = fm.getFunctionCount();
        int ok = 0, done = 0;

        PrintWriter w = new PrintWriter(new FileWriter(outpath));
        try {
            w.println("// Decompiled from " + currentProgram.getName()
                    + " (" + currentProgram.getExecutableFormat() + ")\n");
            FunctionIterator it = fm.getFunctions(true);
            while (it.hasNext()) {
                Function fn = it.next();
                done++;
                try {
                    DecompileResults res = decomp.decompileFunction(fn, 90, monitor);
                    if (res != null && res.decompileCompleted()) {
                        w.println(res.getDecompiledFunction().getC());
                        w.println();
                        ok++;
                    } else {
                        String em = res != null ? res.getErrorMessage() : "no result";
                        w.println("// FAILED " + fn.getName() + " @ " + fn.getEntryPoint() + " : " + em);
                        w.println();
                    }
                } catch (Exception e) {
                    w.println("// EXCEPTION " + fn.getName() + " : " + e.getMessage());
                    w.println();
                }
                if (done % 200 == 0) {
                    println("  ...decompiled " + done + "/" + total);
                }
            }
        } finally {
            w.close();
        }
        println("DecompileExport: " + ok + "/" + total + " functions -> " + outpath);
    }
}

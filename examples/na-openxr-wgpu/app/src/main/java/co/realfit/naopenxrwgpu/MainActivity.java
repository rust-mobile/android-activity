package co.realfit.naopenxrwgpu;

public class MainActivity extends android.app.NativeActivity {
  static {
    System.loadLibrary("openxr_loader");
    System.loadLibrary("main");
  }
}
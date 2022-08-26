package co.realfit.naopenxrinfo;

public class MainActivity extends android.app.NativeActivity {
  static {
    System.loadLibrary("openxr_loader");
    System.loadLibrary("main");
  }
}
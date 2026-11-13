// S26: una ventana de Qt (Widgets) con los controles de siempre, para ver si toma el aspecto de la caja Linux (con
// QT_QPA_PLATFORMTHEME=gtk3, los colores y la letra del tema de GTK 3 de DashOS). Dice cuándo está en pantalla, con su
// paleta, y se cierra sola a los SEGUNDOS de su argumento (20 si no). Licencia MPL-2.0, como el resto de DashOS.
//
//     muestra-qt [SEGUNDOS]
#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QHBoxLayout>
#include <QLineEdit>
#include <QListWidget>
#include <QProgressBar>
#include <QPushButton>
#include <QRadioButton>
#include <QSlider>
#include <QSpinBox>
#include <QTabWidget>
#include <QTimer>
#include <QVBoxLayout>
#include <cstdio>
#include <cstdlib>

static void decir(const QString &t)
{
    std::printf("%s\n", t.toUtf8().constData());
    std::fflush(stdout);
    if (FILE *f = std::fopen("/dev/hvc0", "w")) {
        std::fprintf(f, "%s\n", t.toUtf8().constData());
        std::fclose(f);
    }
}

int main(int argc, char **argv)
{
    int segundos = argc > 1 ? std::atoi(argv[1]) : 20;
    if (segundos <= 0)
        segundos = 20;
    QApplication app(argc, argv);
    QWidget w;
    w.setWindowTitle("Qt en DashOS");
    auto *tabs = new QTabWidget;
    auto *pag = new QWidget;
    auto *col = new QVBoxLayout(pag);
    auto *botones = new QHBoxLayout;
    botones->addWidget(new QPushButton("Normal"));
    auto *aceptar = new QPushButton("Aceptar");
    aceptar->setDefault(true);
    botones->addWidget(aceptar);
    auto *apagado = new QPushButton("Apagado");
    apagado->setEnabled(false);
    botones->addWidget(apagado);
    col->addLayout(botones);
    col->addWidget(new QLineEdit("Ñandú en la caja Linux"));
    auto *fila = new QHBoxLayout;
    fila->addWidget(new QCheckBox("Casilla"));
    auto *marcada = new QCheckBox("Marcada");
    marcada->setChecked(true);
    fila->addWidget(marcada);
    auto *uno = new QRadioButton("Uno");
    uno->setChecked(true);
    fila->addWidget(uno);
    fila->addWidget(new QRadioButton("Dos"));
    col->addLayout(fila);
    auto *esc = new QSlider(Qt::Horizontal);
    esc->setValue(60);
    col->addWidget(esc);
    auto *pb = new QProgressBar;
    pb->setValue(40);
    col->addWidget(pb);
    auto *fila2 = new QHBoxLayout;
    auto *sp = new QSpinBox;
    sp->setValue(42);
    fila2->addWidget(sp);
    auto *combo = new QComboBox;
    combo->addItems({"Pitido", "Destello", "Ninguno"});
    fila2->addWidget(combo);
    col->addLayout(fila2);
    auto *lista = new QListWidget;
    lista->addItems({"Documentos", "Imágenes", "Música"});
    lista->setCurrentRow(1);
    col->addWidget(lista);
    tabs->addTab(pag, "Controles");
    tabs->addTab(new QWidget, "Más");
    auto *raiz = new QVBoxLayout(&w);
    raiz->addWidget(tabs);
    w.resize(560, 470);
    w.show();
    QTimer::singleShot(800, [&] {
        const char *tema = std::getenv("QT_QPA_PLATFORMTHEME");
        decir(QString("muestra-qt: Qt %1 en pantalla, tema de la plataforma %2, ventana %3, resaltado %4, letra %5")
                  .arg(qVersion(), tema ? tema : "(ninguno)", app.palette().window().color().name(),
                       app.palette().highlight().color().name(), app.font().family()));
    });
    QTimer::singleShot(segundos * 1000, [&] {
        decir("muestra-qt: adiós");
        app.quit();
    });
    return app.exec();
}
